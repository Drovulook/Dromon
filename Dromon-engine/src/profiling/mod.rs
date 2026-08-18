//! Profiling par blocs du moteur. Machinerie commune ici ; les drivers (quand vider
//! l'arbre) dans [`initialize`] et [`frame`].
//!
//! [`profile!`] pose un garde RAII ([`ScopeGuard`]) qui mesure dans son `Drop` (donc
//! robuste aux `return`/`?`). Mesures agrégées par `(nom, parent)` : les répétitions
//! sont cumulées (`total_ns` sommé, `count` incrémenté).
//!
//! Accumulation en **nanosecondes** : `as_micros()` tronque vers zéro, donc tout bloc
//! sub-microseconde (un `bind + draw`, ~50 ns) comptait 0 et seuls les rares échantillons
//! aberrants (> 1 µs) apparaissaient. Biais systématique, jamais du bruit centré.
//!
//! Gates : `enabled` (global, `--use-profiling`) et `recording` (thread-local, piloté
//! par les drivers). Sans la feature Cargo `profiling`, la macro s'expanse en néant.

pub mod frame;
pub mod initialize;
pub mod render;

// ─── Implémentation réelle (feature « profiling » active) ─────────────────────────
#[cfg(feature = "profiling")]
mod imp {
    use std::cell::{Cell, RefCell};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Instant;

    static ENABLED: AtomicBool = AtomicBool::new(false);

    pub fn set_enabled(on: bool) {
        ENABLED.store(on, Ordering::Relaxed);
    }

    pub(crate) fn enabled() -> bool {
        ENABLED.load(Ordering::Relaxed)
    }

    thread_local! {
        static RECORDING: Cell<bool> = const { Cell::new(false) };
        // Arène des nœuds (un par (nom, parent)) + pile des nœuds ouverts.
        static NODES: RefCell<Vec<Node>> = const { RefCell::new(Vec::new()) };
        static STACK: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
    }

    pub(crate) fn set_recording(on: bool) {
        RECORDING.with(|r| r.set(on));
    }

    fn recording() -> bool {
        RECORDING.with(|r| r.get())
    }

    /// Nœud agrégé de l'arbre : mêmes `(name, parent)` → même nœud, durées cumulées.
    struct Node {
        name: &'static str,
        parent: Option<usize>, // `None` = racine
        depth: usize,
        total_ns: u128,
        count: u32,
        children: Vec<usize>, // ordre de 1ʳᵉ apparition
    }

    /// Garde RAII de [`crate::profile!`] : son `Drop` accumule la durée sur le nœud.
    pub struct ScopeGuard {
        idx: Option<usize>, // `None` = on n'enregistre pas → `Drop` no-op
        start: Instant,
    }

    impl ScopeGuard {
        pub fn new(name: &'static str) -> Self {
            if !enabled() || !recording() {
                return Self {
                    idx: None,
                    start: Instant::now(),
                };
            }
            let parent = STACK.with(|s| s.borrow().last().copied());
            let idx = NODES.with(|n| {
                let mut nodes = n.borrow_mut();
                // Nœud (nom, parent) déjà vu → on l'agrège, sinon on le crée.
                match nodes
                    .iter()
                    .position(|nd| nd.parent == parent && nd.name == name)
                {
                    Some(i) => i,
                    None => {
                        let depth = parent.map_or(0, |p| nodes[p].depth + 1);
                        let idx = nodes.len();
                        nodes.push(Node {
                            name,
                            parent,
                            depth,
                            total_ns: 0,
                            count: 0,
                            children: Vec::new(),
                        });
                        if let Some(p) = parent {
                            nodes[p].children.push(idx);
                        }
                        idx
                    }
                }
            });
            STACK.with(|s| s.borrow_mut().push(idx));
            Self {
                idx: Some(idx),
                start: Instant::now(),
            }
        }
    }

    impl Drop for ScopeGuard {
        fn drop(&mut self) {
            let Some(idx) = self.idx else { return };
            let dur = self.start.elapsed().as_nanos();
            STACK.with(|s| {
                s.borrow_mut().pop();
            });
            // `get_mut` (pas `[idx]`) : jamais paniquer dans un `Drop`.
            NODES.with(|n| {
                if let Some(nd) = n.borrow_mut().get_mut(idx) {
                    nd.total_ns += dur;
                    nd.count += 1;
                }
            });
        }
    }

    /// Sérialise l'arbre en une ligne (préfixe, racine → enfants) puis le vide.
    /// Nœuds séparés par `\x1e`, champs `depth/dur_ns/count/name` par `\x1f`.
    pub(crate) fn serialize_and_clear() -> String {
        let payload = NODES.with(|n| {
            let nodes = n.borrow();
            let mut records: Vec<String> = Vec::new();
            // Racines en ordre inverse : la pile réinverse → ordre de création.
            let mut stack: Vec<usize> = nodes
                .iter()
                .enumerate()
                .filter(|(_, nd)| nd.parent.is_none())
                .map(|(i, _)| i)
                .rev()
                .collect();
            while let Some(i) = stack.pop() {
                let nd = &nodes[i];
                records.push(format!(
                    "{}\u{1f}{}\u{1f}{}\u{1f}{}",
                    nd.depth, nd.total_ns, nd.count, nd.name
                ));
                for &c in nd.children.iter().rev() {
                    stack.push(c);
                }
            }
            records.join("\u{1e}")
        });
        NODES.with(|n| n.borrow_mut().clear());
        STACK.with(|s| s.borrow_mut().clear());
        payload
    }
}

// ─── Stubs (feature « profiling » absente) ────────────────────────────────────────
// Présents pour que les appels des drivers compilent ; `enabled()` faux les efface.
#[cfg(not(feature = "profiling"))]
mod imp {
    pub fn set_enabled(_on: bool) {}
    pub(crate) fn enabled() -> bool {
        false
    }
    pub(crate) fn set_recording(_on: bool) {}
    pub(crate) fn serialize_and_clear() -> String {
        String::new()
    }
}

#[cfg(feature = "profiling")]
pub use imp::ScopeGuard;
pub use imp::set_enabled;
pub(crate) use imp::{enabled, serialize_and_clear, set_recording};

/// Mesure le temps d'un scope ou d'une fonction.
///
/// - `profile!()` : nom auto (fonction englobante), à poser en 1ʳᵉ ligne du corps.
/// - `profile!("mon bloc")` : nom explicite (littéral `&'static str`), pour un scope.
///
/// Sans la feature `profiling`, s'expanse en néant.
#[cfg(feature = "profiling")]
#[macro_export]
macro_rules! profile {
    () => {
        // Nom auto : `type_name` d'une fn bidon vaut `chemin::…::__dromon_f`, on
        // retire le suffixe `::__dromon_f`.
        let _dromon_profile_guard = {
            fn __dromon_f() {}
            fn __dromon_type_name_of<T>(_: T) -> &'static str {
                ::std::any::type_name::<T>()
            }
            let __name = __dromon_type_name_of(__dromon_f);
            $crate::profiling::ScopeGuard::new(&__name[..__name.len() - "::__dromon_f".len()])
        };
    };
    ($name:expr) => {
        let _dromon_profile_guard = $crate::profiling::ScopeGuard::new($name);
    };
}

/// Variante no-op : sans la feature `profiling`, la macro ne produit aucun code.
#[cfg(not(feature = "profiling"))]
#[macro_export]
macro_rules! profile {
    ($($t:tt)*) => {};
}

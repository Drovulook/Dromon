//! Profiling par blocs pour la phase d'initialisation du moteur.
//!
//! Idée : la macro [`profile!`] pose un garde RAII ([`ScopeGuard`]) au début d'un
//! scope ou d'une fonction. Le garde mesure le temps écoulé dans son `Drop` — donc
//! même en cas de `return` anticipé ou de propagation `?`. Les mesures sont
//! agrégées par `(nom, parent)` dans un arbre *thread-local* : deux ouvertures de
//! même nom sous le même parent sont cumulées (durée additionnée, compteur
//! incrémenté). En fin d'init, [`flush_init`] envoie l'arbre au CLI via le `Logger`.
//!
//! Deux niveaux de contrôle :
//! - **compile-time** : la feature Cargo `profiling`. Absente → la macro s'expanse
//!   en néant et rien de ce module n'entre dans le binaire (voir les stubs plus bas).
//! - **runtime** : le drapeau [`set_enabled`] (armé selon `--use-profiling`). Présent
//!   dans le binaire mais coupé → coût quasi nul (une lecture atomique par scope).

// ─── Implémentation réelle (feature « profiling » active) ─────────────────────────
#[cfg(feature = "profiling")]
mod imp {
    use std::cell::RefCell;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Instant;

    use crate::app::logger::Logger;

    /// Interrupteur global. Tant qu'il est à `false`, les gardes ne mesurent rien
    /// (juste une lecture atomique) : coût négligeable quand le profiling est coupé.
    static ENABLED: AtomicBool = AtomicBool::new(false);

    /// Active/désactive la collecte. Appelé au démarrage selon le flag `--use-profiling`.
    pub fn set_enabled(on: bool) {
        ENABLED.store(on, Ordering::Relaxed);
    }

    fn enabled() -> bool {
        ENABLED.load(Ordering::Relaxed)
    }

    /// Un nœud *agrégé* de l'arbre d'appels. Deux ouvertures de même `name` sous le
    /// même `parent` retombent sur le MÊME nœud : on additionne `total_us` et on
    /// incrémente `count`. C'est ce qui fusionne les répétitions (boucles, appels
    /// multiples d'une même fonction) en une seule ligne cumulée.
    struct Node {
        name: &'static str,
        parent: Option<usize>, // index dans le Vec des nœuds ; `None` = racine
        depth: usize,
        total_us: u128,
        count: u32,
        children: Vec<usize>, // indices des enfants, dans l'ordre de 1ʳᵉ apparition
    }

    thread_local! {
        // Arène de tous les nœuds vus (un par (nom, parent) distinct).
        static NODES: RefCell<Vec<Node>> = const { RefCell::new(Vec::new()) };
        // Pile des nœuds actuellement ouverts : le sommet est le parent des ouvertures.
        static STACK: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
    }

    /// Garde RAII renvoyé par [`crate::profile!`]. Il suffit de le laisser vivre
    /// jusqu'à la fin du scope : c'est son `Drop` qui accumule la durée sur le nœud.
    pub struct ScopeGuard {
        // `None` quand le profiling est désactivé → le `Drop` est un no-op.
        idx: Option<usize>,
        start: Instant,
    }

    impl ScopeGuard {
        pub fn new(name: &'static str) -> Self {
            if !enabled() {
                return Self {
                    idx: None,
                    start: Instant::now(),
                };
            }
            // Parent = nœud ouvert au sommet de la pile (None si on est à la racine).
            let parent = STACK.with(|s| s.borrow().last().copied());
            let idx = NODES.with(|n| {
                let mut nodes = n.borrow_mut();
                // Un nœud (nom, parent) déjà vu ? Alors on l'agrège au lieu d'en créer un.
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
                            total_us: 0,
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
            let dur = self.start.elapsed().as_micros();
            // Les Drop sont LIFO → on dépile, symétrique du `push` du `new`.
            STACK.with(|s| {
                s.borrow_mut().pop();
            });
            // `get_mut` plutôt que `[idx]` : un panic dans un `Drop` est particulièrement
            // dangereux (abort possible en plein unwind).
            NODES.with(|n| {
                if let Some(nd) = n.borrow_mut().get_mut(idx) {
                    nd.total_us += dur;
                    nd.count += 1;
                }
            });
        }
    }

    /// Envoie l'arbre agrégé de la phase d'init au CLI, en parcours préfixe (racine →
    /// enfants, donc directement affichable de haut en bas). À appeler une seule fois,
    /// après la fin de l'init (tous les gardes doivent être droppés). Remet à zéro pour
    /// un éventuel resume ultérieur.
    pub fn flush_init(logger: &Logger) {
        NODES.with(|n| {
            let nodes = n.borrow();
            // Pile de parcours amorcée avec les racines, en ordre inverse pour les
            // ressortir dans l'ordre de création (une pile inverse l'ordre).
            let mut stack: Vec<usize> = nodes
                .iter()
                .enumerate()
                .filter(|(_, nd)| nd.parent.is_none())
                .map(|(i, _)| i)
                .rev()
                .collect();
            while let Some(i) = stack.pop() {
                let nd = &nodes[i];
                logger.profile_init(nd.depth, nd.name, nd.total_us, nd.count);
                for &c in nd.children.iter().rev() {
                    stack.push(c);
                }
            }
        });
        NODES.with(|n| n.borrow_mut().clear());
        STACK.with(|s| s.borrow_mut().clear());
    }
}

// ─── Stubs (feature « profiling » absente) ────────────────────────────────────────
// Rien n'est collecté ; ces fonctions existent seulement pour que les appels
// inconditionnels de `lib.rs`/`App` compilent. La macro, elle, s'expanse en néant.
#[cfg(not(feature = "profiling"))]
mod imp {
    use crate::app::logger::Logger;

    pub fn set_enabled(_on: bool) {}
    pub fn flush_init(_logger: &Logger) {}
}

pub use imp::*;

/// Mesure le temps d'un scope ou d'une fonction.
///
/// - `profile!()` : nomme automatiquement le bloc d'après la fonction englobante.
///   À poser en **première ligne** du corps de la fonction.
/// - `profile!("mon bloc")` : nom explicite (littéral `&'static str`), pour un
///   scope arbitraire.
///
/// Le garde retourné doit vivre jusqu'à la fin du scope ; on le lie donc à une
/// variable que l'on ne touche plus. Sans la feature `profiling`, s'expanse en néant.
#[cfg(feature = "profiling")]
#[macro_export]
macro_rules! profile {
    () => {
        // Astuce classique pour récupérer le chemin de la fonction courante : on
        // déclare une fonction bidon et on lit son `type_name`, qui vaut
        // `chemin::de::la_fonction::__dromon_f`, puis on retire ce suffixe.
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
/// Le `$($t:tt)*` avale n'importe quels arguments (`profile!()` comme `profile!("x")`).
#[cfg(not(feature = "profiling"))]
#[macro_export]
macro_rules! profile {
    ($($t:tt)*) => {};
}

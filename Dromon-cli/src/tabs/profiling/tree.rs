//! Widget d'arbre d'appels réutilisable, partagé par les encadrés « initialize »
//! et « frame » de l'onglet profiling (même format de données côté moteur).
//!
//! Les nœuds arrivent en un lot, en parcours *préfixe* (un parent est toujours
//! avant ses enfants). On reconstruit l'arène dans [`CallTree::load`] à l'aide
//! d'une pile indexée par la profondeur (`depth`). Le lot peut être renvoyé
//! périodiquement (cas des frames, rééchantillonnées) : `load` est idempotent et
//! reconstruit tout à neuf.
//!
//! À l'affichage, chaque nœud visible occupe une ligne : indentation selon la
//! profondeur, chevron de pliage, nom raccourci, durée formatée, puis une barre
//! horizontale proportionnelle à la durée totale (normalisée sur la plus grande
//! durée de l'arbre). Les colonnes durée/barre sont **abandonnées** quand la
//! largeur manque — un petit encadré affiche donc juste le nom, un encadré zoomé
//! le détail complet.

use crate::log_parser::ProfileNode;
use crate::{BLUE, GOLD, MUTED};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem, ListState, Paragraph},
};

/// Un nœud de l'arbre d'appels. `children` référence d'autres nœuds par leur
/// index dans l'arène `nodes` (pas de `Box`/`Rc` : plus simple à manipuler en
/// Rust qu'un arbre à base de pointeurs, et pas de soucis d'emprunt).
struct Node {
    depth: usize,
    name: String,
    total_ns: u128,
    count: u32,
    parent: Option<usize>,
    children: Vec<usize>,
    /// `true` = sous-arbre replié (enfants masqués).
    collapsed: bool,
}

impl Node {
    fn has_children(&self) -> bool {
        !self.children.is_empty()
    }
}

/// État de l'arbre : l'arène des nœuds, les racines, la position du curseur et
/// l'état de défilement de la liste.
pub struct CallTree {
    nodes: Vec<Node>,
    roots: Vec<usize>,
    /// Index du nœud focus dans la liste *visible* (pas dans l'arène).
    cursor: usize,
    list_state: ListState,
    /// Plus grande durée de l'arbre : sert de référence (100 %) pour les barres.
    max_total: u128,
}

impl CallTree {
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            roots: Vec::new(),
            cursor: 0,
            list_state: ListState::default(),
            max_total: 0,
        }
    }

    /// (Re)construit l'arbre à partir du lot complet reçu du moteur. Les nœuds
    /// arrivent en parcours préfixe (parent avant enfants) : une pile d'ancêtres
    /// indexée par profondeur permet de retrouver le parent de chaque nœud —
    /// on dépile tous les nœuds de profondeur >= à la nouvelle, ce qui laisse au
    /// sommet le parent (profondeur - 1), ou rien pour une racine.
    ///
    /// L'état de pliage et la position du curseur sont conservés au mieux (utile
    /// pour les frames renvoyées en boucle : on ne veut pas que l'arbre « saute »
    /// à chaque échantillon). Le pliage est réappliqué par nom de chemin complet.
    pub fn load(&mut self, incoming: Vec<ProfileNode>) {
        // Mémorise les chemins repliés avant de tout reconstruire.
        let collapsed_paths: Vec<String> = self
            .nodes
            .iter()
            .filter(|n| n.collapsed)
            .map(|n| n.name.clone())
            .collect();
        let prev_cursor = self.cursor;

        self.nodes.clear();
        self.roots.clear();
        self.max_total = 0;

        let mut build_stack: Vec<usize> = Vec::new();
        for n in incoming {
            while let Some(&top) = build_stack.last() {
                if self.nodes[top].depth >= n.depth {
                    build_stack.pop();
                } else {
                    break;
                }
            }
            let parent = build_stack.last().copied();
            let idx = self.nodes.len();
            let collapsed = collapsed_paths.contains(&n.name);
            self.nodes.push(Node {
                depth: n.depth,
                name: n.name,
                total_ns: n.dur_ns,
                count: n.count,
                parent,
                children: Vec::new(),
                collapsed,
            });
            match parent {
                Some(p) => self.nodes[p].children.push(idx),
                None => self.roots.push(idx),
            }
            build_stack.push(idx);
            if n.dur_ns > self.max_total {
                self.max_total = n.dur_ns;
            }
        }

        // Reclampe le curseur sur la nouvelle liste visible.
        let visible_len = self.visible().len();
        self.cursor = if visible_len == 0 {
            0
        } else {
            prev_cursor.min(visible_len - 1)
        };
    }

    /// Liste des nœuds actuellement affichés (parcours préfixe, en sautant les
    /// enfants des nœuds repliés). Recalculée à la demande — l'arbre est petit,
    /// inutile de la mettre en cache.
    fn visible(&self) -> Vec<usize> {
        let mut out = Vec::new();
        for &r in &self.roots {
            self.collect(r, &mut out);
        }
        out
    }

    fn collect(&self, idx: usize, out: &mut Vec<usize>) {
        out.push(idx);
        if !self.nodes[idx].collapsed {
            for &c in &self.nodes[idx].children {
                self.collect(c, out);
            }
        }
    }

    /// Monte le curseur d'une ligne (repart en bas si on est tout en haut).
    pub fn move_up(&mut self) {
        let n = self.visible().len();
        if n == 0 {
            return;
        }
        if self.cursor > 0 {
            self.cursor -= 1;
        } else {
            self.cursor = n - 1;
        }
    }

    /// Descend le curseur d'une ligne (repart en haut si on est tout en bas).
    pub fn move_down(&mut self) {
        let n = self.visible().len();
        if n > 0 && self.cursor + 1 < n {
            self.cursor += 1;
        } else {
            self.cursor = 0;
        }
    }

    /// `←` : replie le nœud focus s'il est déplié ; sinon remonte au parent.
    pub fn collapse(&mut self) {
        let visible = self.visible();
        let Some(&idx) = visible.get(self.cursor) else {
            return;
        };
        if self.nodes[idx].has_children() && !self.nodes[idx].collapsed {
            self.nodes[idx].collapsed = true;
        } else if let Some(p) = self.nodes[idx].parent {
            if let Some(pos) = visible.iter().position(|&i| i == p) {
                self.cursor = pos;
            }
        }
    }

    /// `→` : déplie le nœud focus s'il est replié ; sinon descend au 1ᵉʳ enfant.
    pub fn expand(&mut self) {
        let visible = self.visible();
        let Some(&idx) = visible.get(self.cursor) else {
            return;
        };
        if self.nodes[idx].has_children() {
            if self.nodes[idx].collapsed {
                self.nodes[idx].collapsed = false;
            } else {
                self.cursor += 1; // le 1ᵉʳ enfant suit immédiatement en parcours préfixe
            }
        }
    }

    /// `⏎`/`espace` : bascule plié/déplié sur le nœud focus.
    pub fn toggle(&mut self) {
        let visible = self.visible();
        let Some(&idx) = visible.get(self.cursor) else {
            return;
        };
        if self.nodes[idx].has_children() {
            self.nodes[idx].collapsed = !self.nodes[idx].collapsed;
        }
    }

    /// Dessine l'arbre dans `area`. `focused` = l'encadré a le focus (surligne
    /// alors la ligne du curseur). Les colonnes s'adaptent à la largeur : au-delà
    /// du seuil « nom + durée + barre », en dessous « nom + durée », et tout en
    /// bas juste le nom — ainsi un petit encadré reste lisible et un encadré zoomé
    /// montre tout le détail.
    pub fn render(&mut self, frame: &mut Frame, area: Rect, focused: bool) {
        let visible = self.visible();
        if visible.is_empty() {
            let placeholder = Paragraph::new(Span::styled(
                "  en attente de données de profiling…",
                Style::default().fg(MUTED),
            ));
            frame.render_widget(placeholder, area);
            return;
        }
        // Le curseur peut pointer au-delà de la liste après un pliage.
        if self.cursor >= visible.len() {
            self.cursor = visible.len() - 1;
        }

        // Découpage horizontal adaptatif. On réserve `NAME_MIN` colonnes pour un
        // nom lisible et on ajoute durée puis barre seulement si la place reste.
        const NAME_MIN: usize = 16;
        let width = area.width as usize;
        let dur_w = 9usize;

        let show_dur = width >= NAME_MIN + 1 + dur_w;
        let after_dur = width.saturating_sub(if show_dur { dur_w + 1 } else { 0 });
        let bar_cells = if show_dur && after_dur >= NAME_MIN + 1 + 8 {
            (width / 4).clamp(8, 40).min(after_dur - NAME_MIN - 1)
        } else {
            0
        };
        let show_bar = bar_cells > 0;

        let reserved =
            (if show_dur { dur_w + 1 } else { 0 }) + (if show_bar { bar_cells + 1 } else { 0 });
        let name_w = width.saturating_sub(reserved);

        let items: Vec<ListItem> = visible
            .iter()
            .map(|&idx| {
                let node = &self.nodes[idx];

                let indent = "  ".repeat(node.depth);
                let chevron = if node.has_children() {
                    if node.collapsed { "▸ " } else { "▾ " }
                } else {
                    "  "
                };
                let mut label = format!("{indent}{chevron}{}", short_name(&node.name));
                // Un compteur > 1 signale une agrégation (ex. 676 chunks fusionnés).
                if node.count > 1 {
                    label.push_str(&format!(" ×{}", node.count));
                }
                let label = fit(label, name_w);

                let frac = if self.max_total > 0 {
                    node.total_ns as f64 / self.max_total as f64
                } else {
                    0.0
                };

                let mut spans = vec![Span::raw(label)];
                if show_dur {
                    let dur = format!("{:>dur_w$}", fmt_dur(node.total_ns));
                    spans.push(Span::raw(" "));
                    spans.push(Span::styled(dur, Style::default().fg(GOLD)));
                }
                if show_bar {
                    let filled = ((frac * bar_cells as f64).round() as usize).min(bar_cells);
                    spans.push(Span::raw(" "));
                    spans.push(Span::styled(
                        "█".repeat(filled),
                        Style::default().fg(bar_color(frac)),
                    ));
                    spans.push(Span::styled(
                        "░".repeat(bar_cells - filled),
                        Style::default().fg(MUTED),
                    ));
                }
                Line::from(spans)
            })
            .map(ListItem::new)
            .collect();

        // On sélectionne toujours la ligne du curseur pour que le défilement la
        // suive, mais on ne la surligne visuellement que si l'encadré a le focus.
        self.list_state.select(Some(self.cursor));
        let highlight = if focused {
            Style::default()
                .bg(Color::Rgb(0x2A, 0x22, 0x3A))
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        let list = List::new(items).highlight_style(highlight);
        frame.render_stateful_widget(list, area, &mut self.list_state);
    }
}

/// Raccourcit un chemin Rust `a::b::c::Foo::new` en `Foo::new` (deux derniers
/// segments). Les arguments génériques (`Renderer<vulkan::Backend>`) sont d'abord
/// retirés, sinon les `::` qu'ils contiennent fausseraient le découpage. Les noms
/// libres (« create terrain meshes… ») sont laissés intacts.
fn short_name(name: &str) -> String {
    let name = strip_generics(name);
    let parts: Vec<&str> = name.split("::").collect();
    if parts.len() >= 2 {
        format!("{}::{}", parts[parts.len() - 2], parts[parts.len() - 1])
    } else {
        name.clone()
    }
}

/// Supprime tout ce qui est entre chevrons (`<…>`), gestion des imbrications
/// comprise, pour ne garder que le squelette du chemin.
fn strip_generics(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut depth = 0usize;
    for ch in name.chars() {
        match ch {
            '<' => depth += 1,
            '>' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(ch),
            _ => {}
        }
    }
    out
}

/// Tronque (avec « … ») ou complète par des espaces pour occuper exactement `w`
/// colonnes — on raisonne en caractères, pas en octets.
fn fit(s: String, w: usize) -> String {
    let len = s.chars().count();
    if len > w {
        if w == 0 {
            return String::new();
        }
        let mut out: String = s.chars().take(w - 1).collect();
        out.push('…');
        out
    } else {
        let mut out = s;
        out.push_str(&" ".repeat(w - len));
        out
    }
}

/// Formate une durée en nanosecondes vers l'unité la plus lisible.
fn fmt_dur(ns: u128) -> String {
    if ns >= 1_000_000_000 {
        format!("{:.2}s", ns as f64 / 1_000_000_000.0)
    } else if ns >= 1_000_000 {
        format!("{:.1}ms", ns as f64 / 1_000_000.0)
    } else if ns >= 1_000 {
        format!("{:.1}µs", ns as f64 / 1_000.0)
    } else {
        format!("{ns}ns")
    }
}

/// Couleur de la barre selon la part relative : bleu (léger) → or → rouge (lourd).
fn bar_color(frac: f64) -> Color {
    if frac >= 0.66 {
        Color::Rgb(0xE0, 0x6C, 0x75)
    } else if frac >= 0.33 {
        GOLD
    } else {
        BLUE
    }
}

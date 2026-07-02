mod tree;

use tree::CallTree;

use crate::log_parser::ProfileNode;
use crate::{BG, BLUE, BORDER, FG, GOLD};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::Span,
    widgets::{Block, Borders},
};

/// Les trois encadrés de l'onglet, disposés ainsi :
///   initialize        (en haut, pleine largeur)
///   frame    gpu      (en bas, côte à côte)
#[derive(Clone, Copy, PartialEq, Eq)]
enum ProfBlock {
    Initialize,
    Frame,
    Gpu,
}

/// État propre à l'onglet « profiling » : quel encadré est sélectionné, le mode
/// zoom, et les arbres d'appels (initialisation et frame prélevée).
pub struct ProfilingState {
    selected: ProfBlock,
    zoomed: bool,
    initialize: CallTree,
    frame: CallTree,
    gpu: CallTree,
}

impl ProfilingState {
    pub fn new() -> Self {
        Self {
            selected: ProfBlock::Initialize,
            zoomed: false,
            initialize: CallTree::new(),
            frame: CallTree::new(),
            gpu: CallTree::new(),
        }
    }

    /// Charge l'arbre d'initialisation complet reçu du moteur (remplace l'existant).
    pub fn load_init(&mut self, nodes: Vec<ProfileNode>) {
        self.initialize.load(nodes);
    }

    /// Charge l'arbre de la dernière frame prélevée (renvoyée périodiquement).
    pub fn load_frame(&mut self, nodes: Vec<ProfileNode>) {
        self.frame.load(nodes);
    }

    /// Charge l'arbre des temps GPU (renvoyé régulièrement).
    pub fn load_gpu(&mut self, nodes: Vec<ProfileNode>) {
        self.gpu.load(nodes);
    }

    /// L'arbre de l'encadré sélectionné. Les trois encadrés portent désormais un
    /// arbre, mais on garde un `Option` au cas où un futur encadré n'en aurait pas.
    fn selected_tree(&mut self) -> Option<&mut CallTree> {
        match self.selected {
            ProfBlock::Initialize => Some(&mut self.initialize),
            ProfBlock::Frame => Some(&mut self.frame),
            ProfBlock::Gpu => Some(&mut self.gpu),
        }
    }

    // ─── Navigation dans l'arbre (flèches simples) ────────────────────────────
    // Ces actions s'appliquent à l'arbre de l'encadré sélectionné (`initialize`
    // ou `frame`) ; sur `gpu` elles sont sans effet.

    pub fn tree_up(&mut self) {
        if let Some(t) = self.selected_tree() {
            t.move_up();
        }
    }

    pub fn tree_down(&mut self) {
        if let Some(t) = self.selected_tree() {
            t.move_down();
        }
    }

    pub fn tree_left(&mut self) {
        if let Some(t) = self.selected_tree() {
            t.collapse();
        }
    }

    pub fn tree_right(&mut self) {
        if let Some(t) = self.selected_tree() {
            t.expand();
        }
    }

    pub fn on_toggle(&mut self) {
        if let Some(t) = self.selected_tree() {
            t.toggle();
        }
    }

    // ─── Navigation entre encadrés (fn + flèches = Home/End/PageUp/PageDown) ───
    // Grille : `initialize` en haut sur toute la largeur, `frame`/`gpu` en bas.

    pub fn block_up(&mut self) {
        if matches!(self.selected, ProfBlock::Frame | ProfBlock::Gpu) {
            self.selected = ProfBlock::Initialize;
        }
    }

    pub fn block_down(&mut self) {
        if self.selected == ProfBlock::Initialize {
            self.selected = ProfBlock::Frame;
        }
    }

    pub fn block_left(&mut self) {
        if self.selected == ProfBlock::Gpu {
            self.selected = ProfBlock::Frame;
        }
    }

    pub fn block_right(&mut self) {
        if self.selected == ProfBlock::Frame {
            self.selected = ProfBlock::Gpu;
        }
    }

    pub fn toggle_zoom(&mut self) {
        self.zoomed = !self.zoomed;
    }
}

/// Onglet « profiling ». « initialize » (arbre d'appels du démarrage) en haut ;
/// en dessous, « frame » (CPU par frame) et « GPU » côte à côte (encore vides).
/// L'encadré sélectionné a une bordure bleue en gras ; en mode zoom il occupe
/// toute la zone.
pub fn render(frame: &mut Frame, state: &mut ProfilingState, area: Rect) {
    // Mode zoom : le bloc sélectionné occupe toute la zone sous les onglets.
    if state.zoomed {
        match state.selected {
            ProfBlock::Initialize => render_tree(frame, &mut state.initialize, "initialize", area, true),
            ProfBlock::Frame => render_tree(frame, &mut state.frame, "frame", area, true),
            ProfBlock::Gpu => render_tree(frame, &mut state.gpu, "GPU", area, true),
        }
        return;
    }

    let [init_area, bottom_area] =
        Layout::vertical([Constraint::Fill(2), Constraint::Fill(2)]).areas(area);
    let [frame_area, gpu_area] =
        Layout::horizontal([Constraint::Fill(1), Constraint::Fill(1)]).areas(bottom_area);

    // Booléens calculés avant les emprunts mutables des arbres par `render_tree`.
    let init_sel = state.selected == ProfBlock::Initialize;
    let frame_sel = state.selected == ProfBlock::Frame;
    let gpu_sel = state.selected == ProfBlock::Gpu;

    render_tree(frame, &mut state.initialize, "initialize", init_area, init_sel);
    render_tree(frame, &mut state.frame, "frame", frame_area, frame_sel);
    render_tree(frame, &mut state.gpu, "GPU", gpu_area, gpu_sel);
}

/// Dessine un encadré d'arbre (bordure + titre) puis l'arbre à l'intérieur. Le
/// titre rappelle les raccourcis quand l'encadré a le focus. Les colonnes de
/// l'arbre s'adaptent seules à la largeur de `inner` (petit encadré vs zoom).
fn render_tree(frame: &mut Frame, tree: &mut CallTree, name: &str, rect: Rect, focused: bool) {
    let title = if focused {
        format!("{name}  (↑↓ naviguer  ←→ plier/déplier  ⏎ toggle  fn+↑↓←→ encadrés  z zoom)")
    } else {
        name.to_string()
    };
    let block = block_for(&title, focused);
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    tree.render(frame, inner, focused);
}

/// Fabrique un `Block` au thème du CLI, mis en avant s'il a le focus.
fn block_for(title: &str, focused: bool) -> Block<'_> {
    let border_style = if focused {
        Style::default().fg(BLUE).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(BORDER)
    };
    Block::default()
        .title(Span::styled(title, Style::default().fg(GOLD)))
        .borders(Borders::ALL)
        .border_style(border_style)
        .style(Style::default().bg(BG).fg(FG))
}

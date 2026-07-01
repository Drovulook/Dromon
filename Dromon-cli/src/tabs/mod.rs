// Un module par onglet de la barre de navigation. Chaque sous-module expose une
// fonction `render(frame, …, area)` qui dessine le contenu de son onglet dans la
// zone fournie. La barre d'onglets elle-même et les bandeaux du haut (statut,
// config) restent dans `main.rs` car ils sont toujours affichés.
pub mod logs;
pub mod profiling;
pub mod world;

//! Export PNG des champs de la carte macro, pour régler la génération sans lancer
//! le moteur. Nord (+Y) en haut : les lignes de l'image parcourent `j` à l'envers.
//!
//! - `land` : continents, distance à la côte, montagnes, relief ;
//! - `climate` : température, anomalie de SST, précipitations ;
//! - `arrows` : champs de vecteurs (vent, courants) ;
//! - `palette` : couleurs et dégradés partagés ;
//! - `pixels` : rendu cellule par cellule, contours et isolignes.

mod arrows;
mod climate;
mod land;
mod palette;
mod pixels;

pub use arrows::save_arrows;
pub use climate::{save_precipitation, save_sst_anomaly, save_temperature};
pub use land::{relief_image, save_mask, save_relief, save_signed};

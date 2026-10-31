//! **Commandes** reçues du socket (`dromon_ctl.py`, plus tard le pont MCP), exécutées
//! dans la frame. Protocole : une ligne JSON par requête,
//! `{"id": 1, "cmd": "camera.teleport", "args": {…}}` ; réponse au seul client,
//! `[REPLY] {"id": 1, "ok": true, "result": …}` ou `{"id": 1, "ok": false, "error": "…"}`.
//! Une commande qui ne se termine pas dans la frame garde son [`Responder`] pour répondre
//! plus tard.

use crate::app::engine::Engine;
use crate::app::engine::renderer::camera::Camera;
use crate::app::engine::renderer::world::World;
use crate::app::engine::renderer::world::game_clock::GameClock;
use crate::app::socket_server::{Incoming, Replier};
use glam::Vec3;
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::mpsc::Receiver;

#[derive(Deserialize)]
struct Request {
    id: u64,
    #[serde(flatten)]
    command: Command,
}

/// `args` absent accepté pour les commandes sans paramètre. Champ inconnu → erreur
/// (une faute de frappe ne doit pas être ignorée en silence).
#[derive(Deserialize)]
#[serde(tag = "cmd", content = "args", deny_unknown_fields)]
enum Command {
    #[serde(rename = "ping")]
    Ping,
    #[serde(rename = "camera.get")]
    CameraGet,
    /// Angles en degrés, comme `start_yaw_deg` ; omis → inchangés.
    #[serde(rename = "camera.teleport")]
    CameraTeleport {
        pos: Vec3,
        yaw_deg: Option<f32>,
        pitch_deg: Option<f32>,
    },
    #[serde(rename = "time.get")]
    TimeGet,
    /// Fige jour/nuit et animations (vagues, étoiles).
    #[serde(rename = "time.pause")]
    TimePause,
    #[serde(rename = "time.resume")]
    TimeResume,
    /// Heure dans [0, 24), même jour.
    #[serde(rename = "time.set")]
    TimeSet { hour: f64 },
    /// Capture de la fenêtre principale (PNG temporaire). Différée : quelques frames.
    #[serde(rename = "screenshot")]
    Screenshot,
}

/// Réponse due à une requête, envoyée **une seule fois** : `send` consomme le
/// `Responder`. Détruit sans avoir répondu (travail abandonné, moteur arrêté…) → le
/// client reçoit une erreur au lieu d'attendre son délai.
pub(crate) struct Responder {
    id: Value,
    /// `Option` : `Drop` ne reçoit que `&mut self` et doit pouvoir le retirer (`take`).
    reply: Option<Replier>,
}

impl Responder {
    fn new(id: Value, reply: Replier) -> Self {
        Self {
            id,
            reply: Some(reply),
        }
    }

    pub(crate) fn send(mut self, result: Result<Value, String>) {
        self.respond(result);
    }

    fn respond(&mut self, result: Result<Value, String>) {
        let Some(reply) = self.reply.take() else {
            return;
        };
        let response = match result {
            Ok(result) => json!({ "id": self.id, "ok": true, "result": result }),
            Err(error) => json!({ "id": self.id, "ok": false, "error": error }),
        };
        reply.send(format!("[REPLY] {response}"));
    }
}

impl Drop for Responder {
    fn drop(&mut self) {
        self.respond(Err("commande abandonnée sans réponse".to_owned()));
    }
}

impl Engine {
    /// Traite les commandes reçues depuis la frame précédente. Non bloquant.
    pub(super) fn handle_commands(&mut self, requests: &Receiver<Incoming>) {
        while let Ok(Incoming { line, reply }) = requests.try_recv() {
            match parse_request(&line) {
                Ok(Request { id, command }) => {
                    self.execute(command, Responder::new(json!(id), reply));
                }
                Err((id, error)) => Responder::new(id, reply).send(Err(error)),
            }
        }
    }

    fn execute(&mut self, command: Command, responder: Responder) {
        match command {
            // Différées : le responder part avec la demande, la réponse viendra plus tard.
            Command::Screenshot => self.command_screenshot(responder),
            command => responder.send(self.execute_now(command)),
        }
    }

    /// Commandes terminées dans la frame.
    fn execute_now(&mut self, command: Command) -> Result<Value, String> {
        match command {
            Command::Ping => Ok(json!("pong")),
            Command::CameraGet => Ok(camera_pose(&self.world()?.camera)),
            Command::CameraTeleport {
                pos,
                yaw_deg,
                pitch_deg,
            } => {
                let camera = &mut self.world()?.camera;
                camera.teleport(
                    pos,
                    yaw_deg.map(f32::to_radians),
                    pitch_deg.map(f32::to_radians),
                )?;
                Ok(camera_pose(camera))
            }
            Command::TimeGet => Ok(clock_state(&self.world()?.game_clock)),
            Command::TimePause => {
                let clock = &mut self.world()?.game_clock;
                clock.paused = true;
                Ok(clock_state(clock))
            }
            Command::TimeResume => {
                let clock = &mut self.world()?.game_clock;
                clock.paused = false;
                Ok(clock_state(clock))
            }
            Command::TimeSet { hour } => {
                let clock = &mut self.world()?.game_clock;
                clock.set_hour(hour)?;
                Ok(clock_state(clock))
            }
            Command::Screenshot => unreachable!("commande différée, cf. `execute`"),
        }
    }

    /// Le monde de la fenêtre principale.
    fn world(&mut self) -> Result<&mut World, String> {
        self.renderers
            .get_mut(&self.primary_window_id)
            .map(|renderer| renderer.world_mut())
            .ok_or_else(|| "pas de fenêtre principale".to_owned())
    }
}

/// `time_scale` exposé pour qu'on comprenne une heure qui n'avance pas (0 = figée).
fn clock_state(clock: &GameClock) -> Value {
    json!({
        "day": clock.day,
        "hour": clock.hour(),
        "paused": clock.paused,
        "time_scale": clock.time_scale,
    })
}

/// Yaw ramené dans [0, 360[ : la souris le fait dériver sans borne.
fn camera_pose(camera: &Camera) -> Value {
    json!({
        "pos": camera.position,
        "yaw_deg": camera.yaw.to_degrees().rem_euclid(360.0),
        "pitch_deg": camera.pitch.to_degrees(),
        "fov_deg": camera.fov_y.to_degrees(),
    })
}

/// Erreur : `id` de la requête (pour que le client reconnaisse sa réponse) et message.
/// Lue en deux temps (JSON → `Value` → `Request`) : seule une erreur de syntaxe JSON
/// porte une position, inutile pour le reste puisque le client a généré la ligne.
fn parse_request(line: &str) -> Result<Request, (Value, String)> {
    let mut value: Value =
        serde_json::from_str(line).map_err(|e| (Value::Null, e.to_string()))?;
    let id = value.get("id").cloned().unwrap_or(Value::Null);
    match Request::deserialize(&value) {
        Ok(request) => Ok(request),
        // Commande à arguments envoyée sans : serde s'arrête à « missing field `args` ».
        // Avec `args` vide, il nomme le champ qui manque vraiment (« `hour` »).
        Err(_) if value.is_object() && value.get("args").is_none() => {
            value["args"] = json!({});
            Request::deserialize(&value).map_err(|e| (id, e.to_string()))
        }
        Err(e) => Err((id, e.to_string())),
    }
}

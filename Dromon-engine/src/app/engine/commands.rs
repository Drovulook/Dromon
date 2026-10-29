//! **Commandes** reçues du socket (`dromon_ctl.py`, plus tard le pont MCP), exécutées
//! dans la frame. Protocole : une ligne JSON par requête,
//! `{"id": 1, "cmd": "camera.teleport", "args": {…}}` ; réponse au seul client,
//! `[REPLY] {"id": 1, "ok": true, "result": …}` ou `{"id": 1, "ok": false, "error": "…"}`.

use crate::app::engine::Engine;
use crate::app::engine::renderer::camera::Camera;
use crate::app::engine::renderer::world::World;
use crate::app::engine::renderer::world::game_clock::GameClock;
use crate::app::socket_server::Incoming;
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
}

impl Engine {
    /// Traite les commandes reçues depuis la frame précédente. Non bloquant.
    pub(super) fn handle_commands(&mut self, requests: &Receiver<Incoming>) {
        while let Ok(Incoming { line, reply }) = requests.try_recv() {
            let response = match serde_json::from_str::<Request>(&line) {
                Ok(Request { id, command }) => match self.execute(command) {
                    Ok(result) => json!({ "id": id, "ok": true, "result": result }),
                    Err(error) => json!({ "id": id, "ok": false, "error": error }),
                },
                // `id` relu à part pour que le client reconnaisse sa réponse.
                Err(e) => json!({ "id": request_id(&line), "ok": false, "error": e.to_string() }),
            };
            reply.send(format!("[REPLY] {response}"));
        }
    }

    fn execute(&mut self, command: Command) -> Result<Value, String> {
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

fn request_id(line: &str) -> Value {
    serde_json::from_str::<Value>(line)
        .ok()
        .and_then(|request| request.get("id").cloned())
        .unwrap_or(Value::Null)
}

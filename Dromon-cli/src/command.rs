//! Commandes saisies dans la barre de l'onglet logs : même syntaxe que `dromon_ctl.py`,
//! envoyées sur la connexion au moteur. Les réponses (`[REPLY]`) reviennent par le flux
//! des logs (cf. `log_parser`).

use serde_json::{Map, Value, json};
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::sync::{Arc, Mutex, PoisonError};

/// Côté écriture de la connexion au moteur, posé par le thread de lecture à chaque
/// (re)connexion. `None` : moteur absent (compilation, arrêt).
#[derive(Clone, Default)]
pub struct EngineLink(Arc<Mutex<Option<UnixStream>>>);

impl EngineLink {
    pub fn set(&self, stream: Option<UnixStream>) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = stream;
    }

    pub fn send(&self, line: &str) -> Result<(), String> {
        let mut stream = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let stream = stream.as_mut().ok_or("moteur non connecté")?;
        stream
            .write_all(format!("{line}\n").as_bytes())
            .map_err(|e| format!("envoi au moteur : {e}"))
    }
}

/// `camera.teleport pos=1,2,3 yaw_deg=90` → requête JSON.
pub fn build_request(id: u64, input: &str) -> Result<String, String> {
    let mut words = input.split_whitespace();
    let command = words.next().ok_or("commande vide")?;
    let mut args = Map::new();
    for pair in words {
        let (key, value) = pair
            .split_once('=')
            .ok_or_else(|| format!("argument invalide (attendu clé=valeur) : {pair}"))?;
        args.insert(key.to_owned(), parse_value(value));
    }
    let mut request = json!({ "id": id, "cmd": command });
    if !args.is_empty() {
        request["args"] = Value::Object(args);
    }
    Ok(request.to_string())
}

/// JSON si possible, sinon valeurs séparées par des virgules (→ liste), sinon texte.
fn parse_value(text: &str) -> Value {
    if let Ok(value) = serde_json::from_str(text) {
        return value;
    }
    if text.contains(',')
        && let Ok(list) = text.split(',').map(serde_json::from_str).collect()
    {
        return Value::Array(list);
    }
    Value::String(text.to_owned())
}

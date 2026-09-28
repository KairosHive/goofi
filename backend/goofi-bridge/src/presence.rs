//! `/presence`: who is in the patch, and where each pointer is. A socket is a peer for exactly
//! as long as it is open, so the roster is the set of open sockets and nothing expires.
use std::collections::BTreeMap;
use std::sync::Mutex;

use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::sync::broadcast;

/// The peers now present, by id, each with the hue it was dealt on joining.
pub struct Presence {
    next: u64,
    peers: BTreeMap<u64, u16>,
    fanout: broadcast::Sender<String>,
}

impl Default for Presence {
    fn default() -> Presence {
        Presence { next: 1, peers: BTreeMap::new(), fanout: broadcast::channel(256).0 }
    }
}

impl Presence {
    /// Deal the next peer its id and hue. Hues step by the golden angle, so neighbours differ.
    fn join(&mut self) -> (u64, u16) {
        let id = self.next;
        self.next += 1;
        let hue = ((id * 137) % 360) as u16;
        self.peers.insert(id, hue);
        (id, hue)
    }

    fn roster(&self) -> String {
        let peers: Vec<Value> = self.peers.iter().map(|(id, hue)| json!({ "id": id, "hue": hue })).collect();
        json!({ "peers": peers }).to_string()
    }
}

/// One `/presence` socket: `{you, hue}` on arrival, the roster on every join and leave, and each
/// peer's pointer as it moves. A pointer is `{tab, x, y}` in fractions of the window, or `{leave}`.
pub async fn handle(socket: WebSocket, presence: &Mutex<Presence>) {
    let (mut tx, mut rx) = socket.split();
    let (id, hue, mut fanout) = {
        let mut p = presence.lock().unwrap();
        // Subscribed BEFORE the roster is sent, so this peer's own join is the first frame it reads.
        let fanout = p.fanout.subscribe();
        let (id, hue) = p.join();
        let _ = p.fanout.send(p.roster());
        (id, hue, fanout)
    };
    if tx.send(Message::Text(json!({ "you": id, "hue": hue }).to_string().into())).await.is_err() {
        leave(presence, id);
        return;
    }
    loop {
        tokio::select! {
            shared = fanout.recv() => match shared {
                Ok(text) => {
                    if tx.send(Message::Text(text.into())).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => break,
            },
            incoming = rx.next() => match incoming {
                Some(Ok(Message::Text(t))) => {
                    let Ok(v) = serde_json::from_str::<Value>(&t) else { continue };
                    let out = match (v.get("tab"), v.get("x"), v.get("y")) {
                        (Some(tab), Some(x), Some(y)) if tab.is_string() && x.is_number() && y.is_number() => {
                            json!({ "cursor": { "id": id, "hue": hue, "tab": tab, "x": x, "y": y } })
                        }
                        _ => json!({ "gone": id }),
                    };
                    let _ = presence.lock().unwrap().fanout.send(out.to_string());
                }
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                _ => {}
            },
        }
    }
    leave(presence, id);
    let _ = tx.close().await;
}

fn leave(presence: &Mutex<Presence>, id: u64) {
    let mut p = presence.lock().unwrap();
    p.peers.remove(&id);
    let _ = p.fanout.send(json!({ "gone": id }).to_string());
    let _ = p.fanout.send(p.roster());
}

use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use axum::{
    Router,
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
    routing::get,
};
use futures_util::{SinkExt, StreamExt};
use rand::{Rng, distributions::Alphanumeric};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::{
    sync::{Mutex, mpsc},
    time,
};

const ROOM_CODE_LEN: usize = 6;
const MAX_ROOM_PEERS: usize = 2;
const WAITING_ROOM_TTL: Duration = Duration::from_secs(10 * 60);
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PeerProfile {
    pub peer_id: String,
    pub model_mode: String,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ClientMessage {
    CreateRoom {
        profile: PeerProfile,
    },
    JoinRoom {
        room_code: String,
        profile: PeerProfile,
    },
    Activity {
        activity: Value,
    },
    Ping,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ServerMessage {
    RoomCreated { room_code: String },
    RoomJoined { room_code: String },
    PeerJoined { profile: PeerProfile },
    PeerLeft { peer_id: String },
    Activity { peer_id: String, activity: Value },
    Heartbeat,
    Error { code: String, message: String },
}

type Tx = mpsc::UnboundedSender<ServerMessage>;

#[derive(Default)]
pub struct RelayState {
    rooms: Mutex<HashMap<String, Room>>,
}

struct Room {
    created_at: Instant,
    peers: HashMap<String, Peer>,
}

struct Peer {
    profile: PeerProfile,
    tx: Tx,
}

pub fn router() -> Router {
    Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/ws", get(ws_handler))
        .with_state(Arc::new(RelayState::default()))
}

async fn ws_handler(
    State(state): State<Arc<RelayState>>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(state, socket))
}

async fn handle_socket(state: Arc<RelayState>, socket: WebSocket) {
    let (mut sender, mut receiver) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<ServerMessage>();
    let mut session = Session::default();
    let mut heartbeat = time::interval(HEARTBEAT_INTERVAL);

    loop {
        tokio::select! {
            Some(message) = rx.recv() => {
                if send_json(&mut sender, &message).await.is_err() {
                    break;
                }
            }
            Some(incoming) = receiver.next() => {
                let Ok(incoming) = incoming else { break };

                match incoming {
                    Message::Text(text) => {
                        let parsed = serde_json::from_str::<ClientMessage>(&text);

                        let response = match parsed {
                            Ok(message) => process_client_message(&state, &mut session, &tx, message).await,
                            Err(_) => Some(ServerMessage::Error {
                                code: "invalid_message".to_string(),
                                message: "Message is not a valid BongoCat relay payload.".to_string(),
                            }),
                        };

                        if let Some(response) = response {
                            let _ = tx.send(response);
                        }
                    }
                    Message::Close(_) => break,
                    Message::Ping(bytes) => {
                        if sender.send(Message::Pong(bytes)).await.is_err() {
                            break;
                        }
                    }
                    _ => {}
                }
            }
            _ = heartbeat.tick() => {
                let _ = tx.send(ServerMessage::Heartbeat);
            }
            else => break,
        }
    }

    if let (Some(room_code), Some(peer_id)) = (session.room_code, session.peer_id) {
        state.leave_room(&room_code, &peer_id).await;
    }
}

async fn send_json(
    sender: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    message: &ServerMessage,
) -> Result<(), axum::Error> {
    let text = serde_json::to_string(message).expect("server messages must serialize");

    sender.send(Message::Text(text)).await
}

#[derive(Default)]
struct Session {
    room_code: Option<String>,
    peer_id: Option<String>,
}

async fn process_client_message(
    state: &RelayState,
    session: &mut Session,
    tx: &Tx,
    message: ClientMessage,
) -> Option<ServerMessage> {
    match message {
        ClientMessage::CreateRoom { profile } => {
            match state.create_room(profile.clone(), tx.clone()).await {
                Ok(room_code) => {
                    session.room_code = Some(room_code.clone());
                    session.peer_id = Some(profile.peer_id);

                    Some(ServerMessage::RoomCreated { room_code })
                }
                Err(error) => Some(error),
            }
        }
        ClientMessage::JoinRoom { room_code, profile } => {
            match state
                .join_room(&room_code, profile.clone(), tx.clone())
                .await
            {
                Ok(existing_profiles) => {
                    session.room_code = Some(room_code.clone());
                    session.peer_id = Some(profile.peer_id);

                    for profile in existing_profiles {
                        let _ = tx.send(ServerMessage::PeerJoined { profile });
                    }

                    Some(ServerMessage::RoomJoined { room_code })
                }
                Err(error) => Some(error),
            }
        }
        ClientMessage::Activity { activity } => {
            let Some(room_code) = session.room_code.as_deref() else {
                return Some(error(
                    "not_joined",
                    "Join or create a room before sending activity.",
                ));
            };
            let Some(peer_id) = session.peer_id.as_deref() else {
                return Some(error(
                    "not_joined",
                    "Join or create a room before sending activity.",
                ));
            };

            state.broadcast_activity(room_code, peer_id, activity).await;

            None
        }
        ClientMessage::Ping => Some(ServerMessage::Heartbeat),
    }
}

impl RelayState {
    async fn create_room(&self, profile: PeerProfile, tx: Tx) -> Result<String, ServerMessage> {
        let mut rooms = self.rooms.lock().await;
        let room_code = next_room_code(&rooms);

        rooms.insert(
            room_code.clone(),
            Room {
                created_at: Instant::now(),
                peers: HashMap::from([(profile.peer_id.clone(), Peer { profile, tx })]),
            },
        );

        Ok(room_code)
    }

    async fn join_room(
        &self,
        room_code: &str,
        profile: PeerProfile,
        tx: Tx,
    ) -> Result<Vec<PeerProfile>, ServerMessage> {
        let mut rooms = self.rooms.lock().await;
        let Some(room) = rooms.get_mut(room_code) else {
            return Err(error("room_not_found", "Room code was not found."));
        };

        if room.is_expired(Instant::now(), WAITING_ROOM_TTL) {
            rooms.remove(room_code);

            return Err(error("room_expired", "Room code expired."));
        }

        if room.peers.len() >= MAX_ROOM_PEERS && !room.peers.contains_key(&profile.peer_id) {
            return Err(error("room_full", "Room already has two peers."));
        }

        let existing_profiles = room
            .peers
            .values()
            .filter(|peer| peer.profile.peer_id != profile.peer_id)
            .map(|peer| peer.profile.clone())
            .collect::<Vec<_>>();

        for peer in room.peers.values() {
            if peer.profile.peer_id == profile.peer_id {
                continue;
            }

            let _ = peer.tx.send(ServerMessage::PeerJoined {
                profile: profile.clone(),
            });
        }

        room.peers
            .insert(profile.peer_id.clone(), Peer { profile, tx });

        Ok(existing_profiles)
    }

    async fn broadcast_activity(&self, room_code: &str, peer_id: &str, activity: Value) {
        let rooms = self.rooms.lock().await;
        let Some(room) = rooms.get(room_code) else {
            return;
        };

        for peer in room.peers.values() {
            if peer.profile.peer_id == peer_id {
                continue;
            }

            let _ = peer.tx.send(ServerMessage::Activity {
                peer_id: peer_id.to_string(),
                activity: activity.clone(),
            });
        }
    }

    async fn leave_room(&self, room_code: &str, peer_id: &str) {
        let mut rooms = self.rooms.lock().await;
        let Some(room) = rooms.get_mut(room_code) else {
            return;
        };

        room.peers.remove(peer_id);

        for peer in room.peers.values() {
            let _ = peer.tx.send(ServerMessage::PeerLeft {
                peer_id: peer_id.to_string(),
            });
        }

        if room.peers.is_empty() {
            rooms.remove(room_code);
        }
    }
}

impl Room {
    fn is_expired(&self, now: Instant, ttl: Duration) -> bool {
        self.peers.len() < MAX_ROOM_PEERS && now.duration_since(self.created_at) > ttl
    }
}

fn next_room_code(rooms: &HashMap<String, Room>) -> String {
    loop {
        let code = rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(ROOM_CODE_LEN)
            .map(char::from)
            .collect::<String>()
            .to_ascii_uppercase();

        if !rooms.contains_key(&code) {
            return code;
        }
    }
}

fn error(code: &str, message: &str) -> ServerMessage {
    ServerMessage::Error {
        code: code.to_string(),
        message: message.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use serde_json::json;
    use tokio::sync::mpsc;

    use super::*;

    fn profile(peer_id: &str) -> PeerProfile {
        PeerProfile {
            peer_id: peer_id.to_string(),
            model_mode: "standard".to_string(),
        }
    }

    #[tokio::test]
    async fn creates_room_and_joins_second_peer() {
        let state = RelayState::default();
        let (tx_a, mut rx_a) = mpsc::unbounded_channel();
        let (tx_b, _rx_b) = mpsc::unbounded_channel();

        let room_code = state.create_room(profile("a"), tx_a).await.unwrap();
        let existing = state
            .join_room(&room_code, profile("b"), tx_b)
            .await
            .unwrap();

        assert_eq!(existing, vec![profile("a")]);
        assert_eq!(
            rx_a.recv().await,
            Some(ServerMessage::PeerJoined {
                profile: profile("b"),
            })
        );
    }

    #[tokio::test]
    async fn rejects_third_peer() {
        let state = RelayState::default();
        let (tx_a, _rx_a) = mpsc::unbounded_channel();
        let (tx_b, _rx_b) = mpsc::unbounded_channel();
        let (tx_c, _rx_c) = mpsc::unbounded_channel();

        let room_code = state.create_room(profile("a"), tx_a).await.unwrap();
        state
            .join_room(&room_code, profile("b"), tx_b)
            .await
            .unwrap();

        assert_eq!(
            state.join_room(&room_code, profile("c"), tx_c).await,
            Err(error("room_full", "Room already has two peers."))
        );
    }

    #[tokio::test]
    async fn broadcasts_activity_to_other_peer_only() {
        let state = RelayState::default();
        let (tx_a, mut rx_a) = mpsc::unbounded_channel();
        let (tx_b, mut rx_b) = mpsc::unbounded_channel();

        let room_code = state.create_room(profile("a"), tx_a).await.unwrap();
        state
            .join_room(&room_code, profile("b"), tx_b)
            .await
            .unwrap();
        let _ = rx_a.recv().await;

        state
            .broadcast_activity(
                &room_code,
                "a",
                json!({ "kind": "KeyboardPress", "value": "KeyA" }),
            )
            .await;

        assert_eq!(
            rx_b.recv().await,
            Some(ServerMessage::Activity {
                peer_id: "a".to_string(),
                activity: json!({ "kind": "KeyboardPress", "value": "KeyA" }),
            })
        );
        assert!(rx_a.try_recv().is_err());
    }

    #[tokio::test]
    async fn removes_empty_room_after_last_peer_leaves() {
        let state = RelayState::default();
        let (tx_a, _rx_a) = mpsc::unbounded_channel();
        let room_code = state.create_room(profile("a"), tx_a).await.unwrap();

        state.leave_room(&room_code, "a").await;

        let rooms = state.rooms.lock().await;
        assert!(!rooms.contains_key(&room_code));
    }

    #[test]
    fn waiting_room_expires_after_ttl() {
        let room = Room {
            created_at: Instant::now() - Duration::from_secs(601),
            peers: HashMap::from([(
                "a".to_string(),
                Peer {
                    profile: profile("a"),
                    tx: mpsc::unbounded_channel().0,
                },
            )]),
        };

        assert!(room.is_expired(Instant::now(), WAITING_ROOM_TTL));
    }

    #[test]
    fn parses_protocol_messages() {
        let parsed = serde_json::from_value::<ClientMessage>(json!({
            "type": "join_room",
            "roomCode": "ABC123",
            "profile": {
                "peerId": "peer-a",
                "modelMode": "keyboard"
            }
        }))
        .unwrap();

        assert_eq!(
            parsed,
            ClientMessage::JoinRoom {
                room_code: "ABC123".to_string(),
                profile: PeerProfile {
                    peer_id: "peer-a".to_string(),
                    model_mode: "keyboard".to_string(),
                },
            }
        );
        assert!(serde_json::from_value::<ClientMessage>(json!({ "type": "wat" })).is_err());
    }
}

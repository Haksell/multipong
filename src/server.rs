pub mod game {
    tonic::include_proto!("game");
}

use futures::Stream;
use game::{
    game_message::Message,
    game_service_server::{GameService, GameServiceServer},
    ControlInput, GameMessage, GameState, Player,
};
use std::{collections::HashMap, pin::Pin, sync::Arc};
use tokio::sync::{mpsc, Mutex};
use tokio_stream::wrappers::ReceiverStream;
use tonic::{transport::Server, Request, Response, Status};

const SPEED: f32 = 5.0;

type GameMessageStream = Pin<Box<dyn Stream<Item = Result<GameMessage, Status>> + Send>>;

#[derive(Debug, Default)]
struct SharedGameState {
    players: HashMap<i32, Player>,
    clients: HashMap<i32, mpsc::Sender<Result<GameMessage, Status>>>,
    next_player_id: i32,
}

#[derive(Debug, Default)]
pub struct GameServer {
    state: Arc<Mutex<SharedGameState>>,
}

#[tonic::async_trait]
impl GameService for GameServer {
    type PlayStream = GameMessageStream;

    async fn play(
        &self,
        request: Request<tonic::Streaming<GameMessage>>,
    ) -> Result<Response<Self::PlayStream>, Status> {
        let mut stream = request.into_inner();
        let (tx, rx) = mpsc::channel(32);

        let mut state = self.state.lock().await;
        let player_id = state.next_player_id;
        state.next_player_id += 1;

        state.players.insert(
            player_id,
            Player {
                player_id,
                x: 0.0,
                y: 0.0,
            },
        );
        state.clients.insert(player_id, tx.clone());

        let game_state = GameMessage {
            message: Some(Message::GameState(GameState {
                players: state.players.values().cloned().collect(),
            })),
        };
        let _ = tx.send(Ok(game_state.clone())).await;

        for (&id, client) in &state.clients {
            if id != player_id {
                let _ = client.send(Ok(game_state.clone())).await;
            }
        }

        drop(state);

        let state = self.state.clone();

        tokio::spawn(async move {
            while let Ok(Some(game_msg)) = stream.message().await {
                if let Some(Message::ControlInput(ControlInput { dx, dy, player_id })) =
                    game_msg.message
                {
                    let mut state = state.lock().await;

                    if let Some(player) = state.players.get_mut(&player_id) {
                        player.x += SPEED * dx;
                        player.y += SPEED * dy;
                    }

                    let game_state = GameMessage {
                        message: Some(Message::GameState(GameState {
                            players: state.players.values().cloned().collect(),
                        })),
                    };

                    for client in state.clients.values() {
                        let _ = client.send(Ok(game_state.clone())).await;
                    }
                }
            }

            let mut state = state.lock().await;
            state.players.remove(&player_id);
            state.clients.remove(&player_id);

            let game_state = GameMessage {
                message: Some(Message::GameState(GameState {
                    players: state.players.values().cloned().collect(),
                })),
            };

            for client in state.clients.values() {
                let _ = client.send(Ok(game_state.clone())).await;
            }
        });

        let output_stream = ReceiverStream::new(rx);

        Ok(Response::new(Box::pin(output_stream) as Self::PlayStream))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr = "[::1]:50051".parse()?;
    let game_server = GameServer::default();

    println!("Server listening on {}", addr);

    Server::builder()
        .add_service(GameServiceServer::new(game_server))
        .serve(addr)
        .await?;

    Ok(())
}

pub mod game {
    tonic::include_proto!("game");
}

use futures::Stream;
use game::{
    game_message::Message,
    game_service_server::{GameService, GameServiceServer},
    GameMessage, GameState,
};
use std::{pin::Pin, sync::Arc};
use tokio::sync::{mpsc, Mutex};
use tokio_stream::wrappers::ReceiverStream;
use tonic::{transport::Server, Request, Response, Status};

const SPEED: f32 = 5.0;

type GameMessageStream = Pin<Box<dyn Stream<Item = Result<GameMessage, Status>> + Send>>;

#[derive(Debug, Default)]
struct SharedGameState {
    x: f32,
    y: f32,
    clients: Vec<mpsc::Sender<Result<GameMessage, Status>>>,
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

        {
            let mut state = self.state.lock().await;
            state.clients.push(tx.clone());
        }

        let state = self.state.clone();

        tokio::spawn(async move {
            while let Ok(Some(game_msg)) = stream.message().await {
                if let Some(Message::ControlInput(control_input)) = game_msg.message {
                    let mut state = state.lock().await;
                    state.x += SPEED * control_input.dx;
                    state.y += SPEED * control_input.dy;

                    let game_state = GameMessage {
                        message: Some(Message::GameState(GameState {
                            x: state.x,
                            y: state.y,
                        })),
                    };

                    println!("{game_state:?}");

                    for client in &state.clients {
                        let _ = client.send(Ok(game_state.clone())).await;
                    }
                }
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

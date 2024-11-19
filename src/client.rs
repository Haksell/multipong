// TODO: give random paddle to player
// TODO: make game work

pub mod game {
    tonic::include_proto!("game");
}

use bevy::{prelude::*, window::WindowResolution};
use futures::StreamExt;
use game::{
    game_message::Message, game_service_client::GameServiceClient, ControlInput, Direction,
    GameMessage, GameState,
};
use std::{
    sync::{Arc, Mutex},
    thread,
};
use tokio::sync::mpsc::{self, UnboundedSender};
use tokio_stream::wrappers::UnboundedReceiverStream;

#[derive(Resource)]
struct NetworkResource {
    sender: UnboundedSender<GameMessage>,
    game_state: Arc<Mutex<Option<GameState>>>,
}

#[derive(Component)]
struct Paddle {
    is_left: bool,
}

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Pong".into(),
                resolution: WindowResolution::new(800., 800.),
                ..Default::default()
            }),
            ..Default::default()
        }))
        .add_systems(Startup, (setup, network_setup))
        .add_systems(Update, (paddle_input_system, game_state_system))
        .run();
}

fn setup(mut commands: Commands) {
    commands.spawn_empty().insert(Camera2dBundle::default());

    commands
        .spawn(SpriteBundle {
            sprite: Sprite {
                color: Color::WHITE,
                custom_size: Some(Vec2::new(10., 100.)),
                ..Default::default()
            },
            transform: Transform::from_xyz(-380., 0., 0.),
            ..Default::default()
        })
        .insert(Paddle { is_left: true });

    commands
        .spawn(SpriteBundle {
            sprite: Sprite {
                color: Color::WHITE,
                custom_size: Some(Vec2::new(10., 100.)),
                ..Default::default()
            },
            transform: Transform::from_xyz(380., 0., 0.),
            ..Default::default()
        })
        .insert(Paddle { is_left: false });
}

fn network_setup(mut commands: Commands) {
    let (tx, rx) = mpsc::unbounded_channel::<GameMessage>();
    let game_state = Arc::new(Mutex::new(None));

    let network_resource = NetworkResource {
        sender: tx.clone(),
        game_state: game_state.clone(),
    };

    commands.insert_resource(network_resource);

    thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async move {
            let mut client = GameServiceClient::connect("http://[::1]:50051")
                .await
                .expect("Failed to connect to server");

            let outbound = UnboundedReceiverStream::new(rx);

            let response = client
                .play(outbound)
                .await
                .expect("Failed to start Play stream");

            let mut inbound = response.into_inner();

            while let Some(Ok(game_msg)) = inbound.next().await {
                if let Some(Message::GameState(game_state_msg)) = game_msg.message {
                    let mut state = game_state.lock().unwrap();
                    *state = Some(game_state_msg);
                }
            }
        });
    });
}

fn paddle_input_system(
    keyboard_input: Res<ButtonInput<KeyCode>>,
    network: ResMut<NetworkResource>,
) {
    let direction = if keyboard_input.pressed(KeyCode::ArrowUp) {
        Direction::Up
    } else if keyboard_input.pressed(KeyCode::ArrowDown) {
        Direction::Down
    } else {
        Direction::None
    };

    let control_input = ControlInput {
        player_id: 1,
        direction: direction as i32,
    };

    let game_message = GameMessage {
        message: Some(Message::ControlInput(control_input)),
    };

    let _ = network.sender.send(game_message);
}

fn game_state_system(network: Res<NetworkResource>, mut query: Query<(&Paddle, &mut Transform)>) {
    let state_option = { network.game_state.lock().unwrap().clone() };

    if let Some(game_state) = state_option {
        for (paddle, mut transform) in query.iter_mut() {
            if paddle.is_left {
                transform.translation.y = game_state.left_paddle_y;
            } else {
                transform.translation.y = game_state.right_paddle_y;
            }
        }
    }
}

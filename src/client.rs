pub mod game {
    tonic::include_proto!("game");
}

use bevy::{prelude::*, window::WindowResolution};
use futures::StreamExt;
use game::{
    game_message::Message, game_service_client::GameServiceClient, ControlInput, GameMessage,
    Player,
};
use std::{
    collections::HashMap,
    f32::consts::SQRT_2,
    sync::{Arc, Mutex},
    thread,
};
use tokio::sync::mpsc::{self, UnboundedSender};
use tokio_stream::wrappers::UnboundedReceiverStream;

#[derive(Resource)]
struct NetworkResource {
    sender: UnboundedSender<GameMessage>,
    game_state: Arc<Mutex<HashMap<u32, Player>>>,
    player_id: Arc<Mutex<Option<u32>>>,
}

#[derive(Component)]
struct PlayerSquare {
    player_id: u32,
}

const SIZE: f32 = 42.;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "multipong".into(),
                resolution: WindowResolution::new(480., 480.),
                ..Default::default()
            }),
            ..Default::default()
        }))
        .add_systems(Startup, (setup, network_setup))
        .add_systems(Update, (square_input_system, game_state_system))
        .run();
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2dBundle::default());
}

fn network_setup(mut commands: Commands) {
    let (tx, rx) = mpsc::unbounded_channel::<GameMessage>();
    let game_state = Arc::new(Mutex::new(HashMap::new()));
    let player_id = Arc::new(Mutex::new(None));

    let network_resource = NetworkResource {
        sender: tx.clone(),
        game_state: game_state.clone(),
        player_id: player_id.clone(),
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

                    for player in game_state_msg.clone().players {
                        state.insert(player.player_id, player);
                    }

                    let received_ids: Vec<u32> = game_state_msg
                        .clone()
                        .players
                        .iter()
                        .map(|player| player.player_id)
                        .collect();
                    state.retain(|&id, _| received_ids.contains(&id));

                    if player_id.lock().unwrap().is_none() {
                        *player_id.lock().unwrap() = Some(player_id_from_state(&state));
                    }
                }
            }
        });
    });
}

fn player_id_from_state(state: &HashMap<u32, Player>) -> u32 {
    *state.keys().max().unwrap()
}

fn square_input_system(keyboard_input: Res<ButtonInput<KeyCode>>, network: Res<NetworkResource>) {
    let player_id_option = *network.player_id.lock().unwrap();
    if player_id_option.is_none() {
        return;
    }
    let player_id = player_id_option.unwrap();

    let mut dx = keyboard_input.pressed(KeyCode::ArrowRight) as i8 as f32
        - keyboard_input.pressed(KeyCode::ArrowLeft) as i8 as f32;
    let mut dy = keyboard_input.pressed(KeyCode::ArrowUp) as i8 as f32
        - keyboard_input.pressed(KeyCode::ArrowDown) as i8 as f32;
    if dx == 0. && dy == 0. {
        return;
    }
    if dx != 0. && dy != 0. {
        dx /= SQRT_2;
        dy /= SQRT_2;
    }

    let control_input = ControlInput { player_id, dx, dy };

    let game_message = GameMessage {
        message: Some(Message::ControlInput(control_input)),
    };

    let _ = network.sender.send(game_message);
}

fn game_state_system(
    mut commands: Commands,
    network: Res<NetworkResource>,
    mut query: Query<(Entity, &mut Transform, &PlayerSquare)>,
) {
    let state = network.game_state.lock().unwrap().clone();

    let player_id = network.player_id.lock().unwrap();

    let mut existing_players: HashMap<u32, Entity> = HashMap::new();
    for (entity, _, player_square) in query.iter_mut() {
        existing_players.insert(player_square.player_id, entity);
    }

    for (&player_id, &entity) in existing_players.iter() {
        if !state.contains_key(&player_id) {
            commands.entity(entity).despawn();
        }
    }

    for player in state.values() {
        if let Some(&entity) = existing_players.get(&player.player_id) {
            if let Ok((_, mut transform, _)) = query.get_mut(entity) {
                transform.translation.x = player.x;
                transform.translation.y = player.y;
            }
        } else {
            commands
                .spawn(SpriteBundle {
                    sprite: Sprite {
                        color: if Some(player.player_id) == *player_id {
                            Color::srgb(0., 1., 0.)
                        } else {
                            Color::srgb(1., 0., 0.)
                        },
                        custom_size: Some(Vec2::new(SIZE, SIZE)),
                        ..Default::default()
                    },
                    transform: Transform::from_xyz(player.x, player.y, 0.0),
                    ..Default::default()
                })
                .insert(PlayerSquare {
                    player_id: player.player_id,
                });
        }
    }
}

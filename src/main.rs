#![allow(clippy::type_complexity)]

use rand::Rng;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::sync::Arc;
use std::thread;
//use std::time::SystemTime;

use flume::{Receiver, Sender};
//use tracing::info;
use valence::prelude::*;
use valence::spawn::IsFlat;
use valence::CompressionThreshold;
use valence::ServerSettings;

mod config;

use crate::config::read_config;

const HEIGHT: u32 = 384;
const CHUNK_SIZE: i32 = 16;
const CHUNK_CENTER: f64 = (CHUNK_SIZE as f64 - 1.0) / 2.0;

struct ChunkWorkerState {
    sender: Sender<(ChunkPos, UnloadedChunk)>,
    receiver: Receiver<ChunkPos>,
}

#[derive(Resource)]
struct GameState {
    /// Chunks that need to be generated. Chunks without a priority have already
    /// been sent to the thread pool.
    pending: HashMap<ChunkPos, Option<Priority>>,
    sender: Sender<ChunkPos>,
    receiver: Receiver<(ChunkPos, UnloadedChunk)>,
}

/// The order in which chunks should be processed by the thread pool. Smaller
/// values are sent first.
type Priority = u64;

pub fn main() {
    // TODO: arg to specify config file?
    let config: config::Config = read_config("config.toml").unwrap();
    let connection_mode: ConnectionMode = match config.server.connection_mode.as_str() {
        "velocity" => ConnectionMode::Velocity {
            secret: Arc::from(config.server.velocity_secret.clone()),
        },
        "bungeecord" => ConnectionMode::BungeeCord,
        "online" => ConnectionMode::Online {
            prevent_proxy_connections: config.server.online_prevent_proxy_connections,
        },
        "offline" => ConnectionMode::Offline,
        _ => {
            eprintln!("Invalid connection mode: {}", config.server.connection_mode);
            eprintln!("Defaulting to Offline mode");
            ConnectionMode::Offline
        }
    };
    App::new()
        .insert_resource(NetworkSettings {
            connection_mode: connection_mode,
            max_players: config.server.max_players,
            address: config.server.bind_address.parse().unwrap(),
            ..Default::default()
        })
        .insert_resource(ServerSettings {
            compression_threshold: CompressionThreshold(-1),
            ..Default::default()
        })
        .add_plugins(DefaultPlugins)
        .insert_resource(config.world)
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                (
                    init_clients,
                    remove_unviewed_chunks,
                    update_client_views,
                    send_recv_chunks,
                )
                    .chain(),
                despawn_disconnected_clients,
            ),
        )
        .run();
}

fn setup(
    mut commands: Commands,
    server: Res<Server>,
    dimensions: Res<DimensionTypeRegistry>,
    biomes: Res<BiomeRegistry>,
    world_config: Res<config::World>,
) {
    // TODO: put this in player connection?
    /*let seconds_per_day = 86_400;
    let seed = (SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        / seconds_per_day) as u32;

    info!("current seed: {seed}");*/

    let (finished_sender, finished_receiver) = flume::unbounded();
    let (pending_sender, pending_receiver) = flume::unbounded();

    let state = Arc::new(ChunkWorkerState {
        sender: finished_sender,
        receiver: pending_receiver,
    });

    // Chunks are generated in a thread pool for parallelism and to avoid blocking
    // the main tick loop. You can use your thread pool of choice here (rayon,
    // bevy_tasks, etc). Only the standard library is used in the example for the
    // sake of simplicity.
    //
    // If your chunk generation algorithm is inexpensive then there's no need to do
    // this.
    for _ in 0..thread::available_parallelism().unwrap().get() {
        let state = state.clone();
        let world_config = world_config.clone();
        thread::spawn(move || chunk_worker(state, world_config));
    }

    commands.insert_resource(GameState {
        pending: HashMap::new(),
        sender: pending_sender,
        receiver: finished_receiver,
    });

    let layer = LayerBundle::new(ident!("overworld"), &dimensions, &biomes, &server);

    commands.spawn(layer);
}

fn init_clients(
    mut clients: Query<
        (
            &mut EntityLayerId,
            &mut VisibleChunkLayer,
            &mut VisibleEntityLayers,
            &mut Position,
            &mut GameMode,
            &mut IsFlat,
        ),
        Added<Client>,
    >,
    layers: Query<Entity, (With<ChunkLayer>, With<EntityLayer>)>,
    world_config: Res<config::World>,
) {
    for (
        mut layer_id,
        mut visible_chunk_layer,
        mut visible_entity_layers,
        mut pos,
        mut game_mode,
        mut is_flat,
    ) in &mut clients
    {
        let layer = layers.single();

        layer_id.0 = layer;
        visible_chunk_layer.0 = layer;
        visible_entity_layers.0.insert(layer);
        let spawn_pos = DVec3::new(
            world_config.spawn_pos[0],
            world_config.spawn_pos[1],
            world_config.spawn_pos[2],
        );
        pos.set(spawn_pos);
        *game_mode = GameMode::Creative;
        is_flat.0 = true;
    }
}

fn remove_unviewed_chunks(mut layers: Query<&mut ChunkLayer>) {
    layers
        .single_mut()
        .retain_chunks(|_, chunk| chunk.viewer_count_mut() > 0);
}

fn update_client_views(
    mut layers: Query<&mut ChunkLayer>,
    mut clients: Query<(&mut Client, View, OldView)>,
    mut state: ResMut<GameState>,
) {
    let layer = layers.single_mut();

    for (client, view, old_view) in &mut clients {
        let view = view.get();
        let queue_pos = |pos: ChunkPos| {
            if layer.chunk(pos).is_none() {
                match state.pending.entry(pos) {
                    Entry::Occupied(mut oe) => {
                        if let Some(priority) = oe.get_mut() {
                            let dist = view.pos.distance_squared(pos);
                            *priority = (*priority).min(dist);
                        }
                    }
                    Entry::Vacant(ve) => {
                        let dist = view.pos.distance_squared(pos);
                        ve.insert(Some(dist));
                    }
                }
            }
        };

        // Queue all the new chunks in the view to be sent to the thread pool.
        if client.is_added() {
            view.iter().for_each(queue_pos);
        } else {
            let old_view = old_view.get();
            if old_view != view {
                view.diff(old_view).for_each(queue_pos);
            }
        }
    }
}

fn send_recv_chunks(mut layers: Query<&mut ChunkLayer>, state: ResMut<GameState>) {
    let mut layer = layers.single_mut();
    let state = state.into_inner();

    // Insert the chunks that are finished generating into the instance.
    for (pos, chunk) in state.receiver.drain() {
        layer.insert_chunk(pos, chunk);
        assert!(state.pending.remove(&pos).is_some());
    }

    // Collect all the new chunks that need to be loaded this tick.
    let mut to_send = vec![];

    for (pos, priority) in &mut state.pending {
        if let Some(pri) = priority.take() {
            to_send.push((pri, pos));
        }
    }

    // Sort chunks by ascending priority.
    to_send.sort_unstable_by_key(|(pri, _)| *pri);

    // Send the sorted chunks to be loaded.
    for (_, pos) in to_send {
        let _ = state.sender.try_send(*pos);
    }
}

fn chunk_worker(state: Arc<ChunkWorkerState>, world_config: config::World) {
    while let Ok(pos) = state.receiver.recv() {
        let mut chunk = UnloadedChunk::with_height(HEIGHT);
        let can_be_wall: [[bool; 2]; 2] = [
            [
                random_bool(world_config.wall_chance),
                random_bool(world_config.wall_chance),
            ],
            [
                random_bool(world_config.wall_chance),
                random_bool(world_config.wall_chance),
            ],
        ];

        for offset_z in 0..16 {
            for offset_x in 0..16 {
                let x = offset_x as i32 + pos.x * 16;
                let z = offset_z as i32 + pos.z * 16;

                // Fill in the terrain column.
                for offset_y in (0..chunk.height() as i32).rev() {
                    let y = offset_y - 64;
                    // TODO: Chunk logic
                    let mut block: BlockState = BlockState::AIR;

                    for center in &world_config.center_map {
                        if y >= center.min_y
                            && y < center.max_y
                            && ((offset_x as f64 - CHUNK_CENTER).abs() <= center.radius as f64)
                            && ((offset_z as f64 - CHUNK_CENTER).abs() <= center.radius as f64)
                        {
                            block = BlockState::from_kind(
                                BlockKind::from_str(center.mat.as_str()).unwrap_or_default(),
                            );
                            break;
                        }
                    }

                    if block == (BlockState::AIR) {
                        for wall in &world_config.wall_map {
                            if y >= wall.min_y
                                && y < wall.max_y
                                && determine_wall(
                                    offset_x,
                                    offset_z,
                                    can_be_wall,
                                    wall.thick as i32,
                                )
                            {
                                block = BlockState::from_kind(
                                    BlockKind::from_str(wall.mat.as_str()).unwrap_or_default(),
                                );
                                break;
                            }
                        }
                    }

                    if block == (BlockState::AIR) {
                        for layer in &world_config.layer_map {
                            if y == layer.y {
                                block = BlockState::from_kind(
                                    BlockKind::from_str(layer.mat.as_str()).unwrap_or_default(),
                                );
                                break;
                            }
                        }
                    };

                    chunk.set_block_state(offset_x as u32, offset_y as u32, offset_z as u32, block);
                }
            }
        }

        let _ = state.sender.try_send((pos, chunk));
    }
}

fn random_bool(chance: f64) -> bool {
    let mut rng = rand::thread_rng();
    rng.gen::<f64>() <= chance
}

fn determine_wall(
    offset_x: i32,
    offset_z: i32,
    can_be_wall: [[bool; 2]; 2],
    thickness: i32,
) -> bool {
    let mut is_wall: bool = false;
    is_wall = is_wall || ((offset_x < thickness) && can_be_wall[0][0]);
    is_wall = is_wall || ((offset_x >= CHUNK_SIZE - thickness) && can_be_wall[0][1]);
    is_wall = is_wall || ((offset_z < thickness) && can_be_wall[1][0]);
    is_wall = is_wall || ((offset_z >= CHUNK_SIZE - thickness) && can_be_wall[1][1]);
    is_wall
}

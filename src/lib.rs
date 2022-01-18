use std::collections::vec_deque::VecDeque;

use libcraft_core::{vec3, EntityKind};
use quill::components::Name;
use quill::entities::{Player, Tnt};
use quill::events::{EntityRemoveEvent, GamemodeEvent, PlayerJoinEvent};
use quill::{
    BlockPosition, BlockState, Entity, EntityId, EntityInit, Game, Gamemode, Plugin, Position,
    Setup, TextComponent, TextComponentBuilder, Title,
};
use rand::Rng;

const PREPARATION_TIME: usize = 5;
const RESULTS_TIME: usize = 5;
const BLOCK_FALL_DELAY: usize = 5;
const FALLING_BLOCK_DESPAWN_Y: f64 = 0.0;
const LOSE_Y: f64 = 5.0;
const SPAWN_CENTER: Position = Position {
    x: 0.0,
    y: 22.0,
    z: 0.0,
    pitch: 0.0,
    yaw: 0.0,
};
const SPAWN_RADIUS: f64 = 10.0;
const LAYER_RADIUS: usize = 15;
const LAYERS: &[i32] = &[20, 15, 10]; 
const BLOCK_STATE_ID: u16 = 1430;
const SPECTATOR_SPAWN_POINT: Position = Position {
    x: 0.0,
    y: 27.0,
    z: 0.0,
    pitch: 0.0,
    yaw: 90.0,
};

quill::plugin!(TntRun);

struct TntRun {
    state: TntRunState,
    tick_counter: usize,
}

impl Plugin for TntRun {
    fn enable(_game: &mut Game, setup: &mut Setup<Self>) -> Self {
        // FIXME regenerate_arena(game) fails because the chunk is not loaded
        setup
            .add_system(player_join_system)
            .add_system(start_system)
            .add_system(remove_offline_players_system)
            .add_system(tick_counter_system)
            .add_system(block_queue_system)
            .add_system(block_fall_system)
            .add_system(block_falling_system)
            .add_system(lose_system)
            .add_system(winner_system);
        TntRun {
            state: TntRunState::Waiting {
                countdown: 0,
            },
            tick_counter: 0,
        }
    }

    fn disable(self, _game: &mut Game) {}
}

fn player_join_system(plugin: &mut TntRun, game: &mut Game) {
    for (player, _) in game.query::<&PlayerJoinEvent>() {
        match plugin.state {
            TntRunState::Starting { .. } => respawn_in_arena(&player),
            TntRunState::Playing { .. } | TntRunState::Waiting { .. } => {
                respawn_as_spectator(&player);
            }
        }
    }
}

fn tick_counter_system(plugin: &mut TntRun, _game: &mut Game) {
    plugin.tick_counter += 1;
}

fn start_system(plugin: &mut TntRun, game: &mut Game) {
    if plugin.tick_counter % 20 != 0 {
        return;
    }
    match &mut plugin.state {
        TntRunState::Waiting { countdown } => {
            if *countdown == 0 {
                let players = game.query::<&Player>().map(|(e, _)| e).collect::<Vec<_>>();
                if !players.is_empty() {
                    plugin.state = TntRunState::Starting {
                        countdown: PREPARATION_TIME,
                    };
                    for player in players {
                        respawn_in_arena(&player)
                    }
                    regenerate_arena(game);
                }
            } else {
                *countdown -= 1;
            }
        }
        TntRunState::Starting { countdown } => {
            let players = game
                .query::<&Player>()
                .map(|(entity, _)| entity)
                .collect::<Vec<_>>();
            if players.len() > 1 {
                *countdown -= 1;
                if *countdown == 0 {
                    plugin.state = TntRunState::Playing {
                        block_fall_queue: VecDeque::new(),
                        falling_blocks: Vec::new(),
                        players: players.iter().map(|p| p.id()).collect(),
                    };
                    for player in players {
                        player.send_title(&Title {
                            title: Some(TextComponent::from("Run!").red().into()),
                            sub_title: None,
                            fade_in: 0,
                            stay: 10,
                            fade_out: 5,
                        });
                    }
                } else {
                    for player in players {
                        player.send_title(&Title {
                            title: Some(TextComponent::from(countdown.to_string()).green().into()),
                            sub_title: Some("Get ready".into()),
                            fade_in: 1,
                            stay: 20,
                            fade_out: 1,
                        });
                    }
                }
            } else {
                if *countdown != PREPARATION_TIME {
                    for player in players {
                        player.send_title(&Title {
                            title: Some("Preparation interrupted".into()),
                            sub_title: Some("Waiting for other players to join".into()),
                            fade_in: 5,
                            stay: 90,
                            fade_out: 5,
                        });
                    }
                }
                *countdown = PREPARATION_TIME;
            }
        }
        _ => (),
    }
}

fn remove_offline_players_system(plugin: &mut TntRun, game: &mut Game) {
    if let TntRunState::Playing { players, .. } = &mut plugin.state {
        players.retain(|player| {
            game.entity(*player).is_ok()
                && game
                    .entity(*player)
                    .unwrap()
                    .get::<EntityRemoveEvent>()
                    .is_err()
        });
    }
}

fn block_queue_system(plugin: &mut TntRun, game: &mut Game) {
    if let TntRunState::Playing {
        block_fall_queue, ..
    } = &mut plugin.state
    {
        for (_player, (_, position)) in game.query::<(&Player, &Position)>() {
            let pos1 = position
                + vec3(
                    EntityKind::Player.bounding_box().into_rect3().w / 2.0,
                    0.0,
                    EntityKind::Player.bounding_box().into_rect3().w / 2.0,
                );
            let pos2 = position
                + vec3(
                    -EntityKind::Player.bounding_box().into_rect3().w / 2.0,
                    0.0,
                    EntityKind::Player.bounding_box().into_rect3().w / 2.0,
                );
            let pos3 = position
                + vec3(
                    EntityKind::Player.bounding_box().into_rect3().w / 2.0,
                    0.0,
                    -EntityKind::Player.bounding_box().into_rect3().w / 2.0,
                );
            let pos4 = position
                + vec3(
                    -EntityKind::Player.bounding_box().into_rect3().w / 2.0,
                    0.0,
                    -EntityKind::Player.bounding_box().into_rect3().w / 2.0,
                );
            for position in [pos1, pos2, pos3, pos4] {
                let block_pos = position.block().down();
                if game.block(block_pos).map_or(false, |block| block.id() == BLOCK_STATE_ID)
                    && block_fall_queue
                        .iter()
                        .rfind(|(_, pos)| *pos == block_pos)
                        .is_none()
                {
                    block_fall_queue.push_back((plugin.tick_counter + BLOCK_FALL_DELAY, block_pos));
                }
            }
        }
    }
}

fn block_fall_system(plugin: &mut TntRun, game: &mut Game) {
    if let TntRunState::Playing {
        block_fall_queue,
        falling_blocks,
        ..
    } = &mut plugin.state
    {
        let count = block_fall_queue
            .iter()
            .position(|(time, _)| *time > plugin.tick_counter)
            .unwrap_or(block_fall_queue.len());
        for (_, block_pos) in block_fall_queue.drain(..count) {
            game.set_block(block_pos, BlockState::from_id(0).unwrap())
                .unwrap();
            // FIXME this doesn't work
            let entity = game
                .create_entity_builder(block_pos.position(), EntityInit::Tnt)
                .finish();
            falling_blocks.push(entity.id());
        }
    }
}

fn block_falling_system(plugin: &mut TntRun, game: &mut Game) {
    if let TntRunState::Playing { falling_blocks, .. } = &mut plugin.state {
        for (entity, (mut position, _)) in game.query::<(&mut Position, &Tnt)>() {
            if falling_blocks.contains(&entity.id()) {
                position.y -= 0.01;
                // I'm not sure despawning entities is possible with quill
                if position.y <= FALLING_BLOCK_DESPAWN_Y {
                    falling_blocks.remove(
                        falling_blocks
                            .iter()
                            .position(|e| *e == entity.id())
                            .unwrap(),
                    );
                }
            }
        }
    }
}

fn lose_system(plugin: &mut TntRun, game: &mut Game) {
    if let TntRunState::Playing { players, .. } = &mut plugin.state {
        let mut lost = Vec::with_capacity(players.len());
        for player in &*players {
            let player = game.entity(*player).unwrap();
            if player.get::<Position>().unwrap().y <= LOSE_Y {
                player.send_title(&Title {
                    title: Some(TextComponent::from("You lose").red().into()),
                    sub_title: None,
                    fade_in: 1,
                    stay: 10,
                    fade_out: 8,
                });
                respawn_as_spectator(&player);
                lost.push(player.id());
            }
        }
        players.retain(|player| !lost.contains(player));
    }
}

fn winner_system(plugin: &mut TntRun, game: &mut Game) {
    if let TntRunState::Playing { players, .. } = &mut plugin.state {
        if players.len() == 1 {
            let winner = game.entity(players[0]).unwrap();
            for (entity, _) in game.query::<&Player>() {
                let text = if entity.id() == winner.id() {
                    respawn_as_spectator(&entity);
                    TextComponent::from("You won!").green().into()
                } else {
                    TextComponent::from(format!("{} won!", winner.get::<Name>().unwrap()))
                        .yellow()
                        .into()
                };
                entity.send_title(&Title {
                    title: Some(text),
                    sub_title: None,
                    fade_in: 1,
                    stay: 70,
                    fade_out: 10,
                });
            }
            plugin.state = TntRunState::Waiting {
                countdown: RESULTS_TIME,
            }
        } else if players.is_empty() {
            for (entity, _) in game.query::<&Player>() {
                entity.send_title(&Title {
                    title: Some(TextComponent::from("Draw!").yellow().into()),
                    sub_title: None,
                    fade_in: 1,
                    stay: 20,
                    fade_out: 10,
                });
            }
            plugin.state = TntRunState::Waiting {
                countdown: RESULTS_TIME,
            }
        }
    }
}

enum TntRunState {
    Starting {
        countdown: usize,
    },
    Playing {
        block_fall_queue: VecDeque<(usize, BlockPosition)>,
        falling_blocks: Vec<EntityId>,
        players: Vec<EntityId>,
    },
    Waiting {
        countdown: usize,
    },
}

fn respawn_as_spectator(player: &Entity) {
    player.insert_event(GamemodeEvent(Gamemode::Spectator));
    player.insert(SPECTATOR_SPAWN_POINT);
}

fn respawn_in_arena(player: &Entity) {
    let mut rng = rand::thread_rng();
    player.insert_event(GamemodeEvent(Gamemode::Adventure));
    let mut spawn = SPAWN_CENTER;
    spawn.x += rng.gen_range((-SPAWN_RADIUS)..SPAWN_RADIUS);
    spawn.z += rng.gen_range((-SPAWN_RADIUS)..SPAWN_RADIUS);
    player.insert(spawn);
}

fn regenerate_arena(game: &Game) {
    for x in (-(LAYER_RADIUS as i32))..=(LAYER_RADIUS as i32) {
        for z in (-(LAYER_RADIUS as i32))..=(LAYER_RADIUS as i32) {
            for layer_y in LAYERS {
                game.set_block(
                    BlockPosition::new(x, *layer_y, z),
                    BlockState::from_id(BLOCK_STATE_ID).unwrap(),
                )
                    .unwrap();
            }
        }
    }
}

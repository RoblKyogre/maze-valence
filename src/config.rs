use serde::Deserialize;
use std::error::Error;
use std::fs;
use std::path::PathBuf;
use valence::prelude::*;

#[derive(Deserialize, Clone)]
pub struct Config {
    pub server: Server,
    pub world: World,
}

#[derive(Deserialize, Clone)]
pub struct Server {
    pub bind_address: String,
    pub max_players: usize,
    pub connection_mode: String,
    pub online_prevent_proxy_connections: bool,
    pub velocity_secret: String,
}

#[derive(Deserialize, Clone, valence::prelude::Resource)]
pub struct World {
    pub center_map: Vec<Center>,
    pub wall_map: Vec<Wall>,
    pub layer_map: Vec<Layer>,
    pub spawn_pos: Vec<f64>,
    pub wall_chance: f64,
    pub login_message: String,
    pub biome: String,
}

#[derive(Deserialize, Clone)]
pub struct Layer {
    pub mat: String,
    pub y: i32,
}

#[derive(Deserialize, Clone)]
pub struct Wall {
    pub mat: String,
    pub min_y: i32,
    pub max_y: i32,
    pub thick: u32,
}

#[derive(Deserialize, Clone)]
pub struct Center {
    pub mat: String,
    pub min_y: i32,
    pub max_y: i32,
    pub radius: u32,
}

pub fn read_config(file_path: PathBuf) -> Result<Config, Box<dyn Error>> {
    let toml_content = fs::read_to_string(file_path)?;
    let config: Config = toml::from_str(&toml_content)?;

    Ok(config)
}

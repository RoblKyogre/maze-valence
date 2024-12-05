use serde::Deserialize;
use std::error::Error;
use std::fs;
use valence::prelude::*;

#[derive(Deserialize, Clone, valence::prelude::Resource)]
pub struct Config {
    pub server: Server,
    pub generator: Generator,
}

#[derive(Deserialize, Clone)]
pub struct Server {
    pub bind_address: String,
    pub max_players: usize,
    pub connection_mode: String,
    pub online_prevent_proxy_connections: bool,
    pub velocity_secret: String,
}

#[derive(Deserialize, Clone)]
pub struct Generator {
    pub layer_map: Vec<Layer>,
    pub wall: Wall,
    pub spawn_pos: Vec<f64>,
}

#[derive(Deserialize, Clone)]
pub struct Layer {
    pub mat: u16,
    pub y: i32,
}

#[derive(Deserialize, Clone)]
pub struct Wall {
    pub mat: u16,
    pub max_y: i32,
    pub thick: u32,
}

pub fn read_config(file_path: &str) -> Result<Config, Box<dyn Error>> {
    let toml_content = fs::read_to_string(file_path)?;
    let config: Config = toml::from_str(&toml_content)?;

    Ok(config)
}

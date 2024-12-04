use serde::Deserialize;
use std::error::Error;
use std::fs;

#[derive(Deserialize)]
pub struct Config {
    pub server: Server,
    pub generator: Generator,
}

#[derive(Deserialize)]
pub struct Server {
    pub bind_address: String,
    pub max_players: u32,
    pub connection_mode: String,
    pub velocity_secret: String,
}

#[derive(Deserialize)]
pub struct Generator {
    pub layer_map: Vec<Layer>,
    pub wall_thickness: u32,
    pub wall_material: String,
}

#[derive(Deserialize)]
pub struct Layer {
    pub mat: String,
    pub y: i32,
}

pub fn read_config(file_path: &str) -> Result<Config, Box<dyn Error>> {
    let toml_content = fs::read_to_string(file_path)?;
    let config: Config = toml::from_str(&toml_content)?;

    Ok(config)
}

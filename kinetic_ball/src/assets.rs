use bevy::asset::uuid_handle;
use bevy::asset::RenderAssetUsages;
use bevy::image::{CompressedImageFormats, ImageSampler, ImageType};
use bevy::prelude::*;
use bevy::shader::Shader;

// ============================================================================
// ASSETS EMBEBIDOS EN EL BINARIO
// ============================================================================

pub const BALL_PNG: &[u8] = include_bytes!("../assets/ball.png");
pub const DEFAULT_MAP: &str = include_str!("../assets/cancha_grande.hbs");
pub const SPLIT_SCREEN_SHADER_SRC: &str =
    include_str!("../assets/shaders/split_screen_compositor.wgsl");

// Handle constante para el shader de split-screen (usando UUID fijo)
pub const SPLIT_SCREEN_SHADER_HANDLE: Handle<Shader> =
    uuid_handle!("1a2b3c4d-5e6f-7890-abcd-ef1234567890");

/// Assets embebidos cargados en memoria
#[derive(Resource, Default)]
pub struct EmbeddedAssets {
    pub ball_texture: Handle<Image>,
}

/// Carga los assets embebidos en memoria al iniciar la aplicación
pub fn load_embedded_assets(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut shaders: ResMut<Assets<Shader>>,
) {
    let ball_image = Image::from_buffer(
        BALL_PNG,
        ImageType::Extension("png"),
        CompressedImageFormats::NONE,
        true,
        ImageSampler::default(),
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    )
    .expect("Failed to load embedded ball.png");

    let ball_handle = images.add(ball_image);

    // Cargar el shader de split-screen embebido
    shaders.insert(
        &SPLIT_SCREEN_SHADER_HANDLE,
        Shader::from_wgsl(SPLIT_SCREEN_SHADER_SRC, file!()),
    );

    commands.insert_resource(EmbeddedAssets {
        ball_texture: ball_handle,
    });

    println!("✅ Assets embebidos cargados en memoria");
}

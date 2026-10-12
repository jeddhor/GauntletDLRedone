//! Native original-data texture laboratory; no gameplay milestone is claimed.
use bevy::{
    asset::RenderAssetUsages,
    image::ImageSampler,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use gdl_formats::texdef::TextureTable;
use sha2::{Digest, Sha256};
use std::{error::Error, fs, path::PathBuf};

#[derive(Resource)]
struct Gallery {
    table: TextureTable,
    pixels: Vec<u8>,
    selected: usize,
    alpha_mode: u8,
    texdef_hash: String,
    textures_hash: String,
}
#[derive(Component)]
struct Preview;
#[derive(Component)]
struct Description;

fn create_preview(gallery: &Gallery, images: &mut Assets<Image>) -> (Handle<Image>, Vec2, String) {
    let b = &gallery.table.bitmaps[gallery.selected];
    let mode = match gallery.alpha_mode {
        0 => "source alpha",
        1 => "opaque RGB diagnostic",
        _ => "GS 0..128 alpha hypothesis",
    };
    match b.decode_base_rgba(&gallery.pixels) {
        Ok(mut rgba) => {
            for pixel in rgba.chunks_exact_mut(4) {
                if gallery.alpha_mode == 1 {
                    pixel[3] = 255;
                } else if gallery.alpha_mode == 2 {
                    pixel[3] = ((pixel[3] as u16 * 255) / 128).min(255) as u8;
                }
            }
            let mut image = Image::new(
                Extent3d {
                    width: b.width as u32,
                    height: b.height as u32,
                    depth_or_array_layers: 1,
                },
                TextureDimension::D2,
                rgba,
                TextureFormat::Rgba8UnormSrgb,
                RenderAssetUsages::default(),
            );
            image.sampler = ImageSampler::nearest();
            let scale = (680.0 / b.width as f32)
                .min(480.0 / b.height as f32)
                .min(4.0);
            let size = Vec2::new(b.width as f32, b.height as f32) * scale;
            (
                images.add(image),
                size,
                format!(
                    "{} / {}  {}\n{}x{}  format {}  flags {:#06x}  mipmaps {}\n{}\nTEXDEF {}  TEXTURES {}",
                    b.index + 1,
                    gallery.table.bitmaps.len(),
                    b.label,
                    b.width,
                    b.height,
                    b.format,
                    b.flags,
                    b.mipmap_count,
                    mode,
                    &gallery.texdef_hash[..12],
                    &gallery.textures_hash[..12]
                ),
            )
        }
        Err(e) => (
            Handle::default(),
            Vec2::ZERO,
            format!(
                "{} / {}  {}\nUNSUPPORTED: {}\nRaw record is retained; no replacement pixels.",
                b.index + 1,
                gallery.table.bitmaps.len(),
                b.label,
                e
            ),
        ),
    }
}

fn setup(mut commands: Commands, gallery: Res<Gallery>, mut images: ResMut<Assets<Image>>) {
    commands.spawn(Camera2d);
    for y in -8..8 {
        for x in -12..12 {
            let v = if (x + y) % 2 == 0 { 0.16 } else { 0.21 };
            commands.spawn((
                Sprite::from_color(Color::srgb(v, v, v), Vec2::splat(32.0)),
                Transform::from_xyz(x as f32 * 32.0 + 16.0, y as f32 * 32.0 + 16.0, -1.0),
            ));
        }
    }
    let (image, size, label) = create_preview(&gallery, &mut images);
    commands.spawn((
        Sprite {
            image,
            custom_size: Some(size),
            ..default()
        },
        Preview,
    ));
    commands.spawn((
        Text2d::new(label),
        TextFont::from_font_size(18.0),
        TextColor(Color::WHITE),
        Transform::from_xyz(0.0, 310.0, 2.0),
        Description,
    ));
    commands.spawn((Text2d::new("Original-data texture inspection - gameplay not implemented\nLeft / Right: bitmap   A: alpha diagnostic   Esc: close"),
        TextFont::from_font_size(16.0),TextColor(Color::WHITE),Transform::from_xyz(0.0,-300.0,2.0)));
}

fn browse(
    keys: Res<ButtonInput<KeyCode>>,
    mut gallery: ResMut<Gallery>,
    mut images: ResMut<Assets<Image>>,
    mut preview: Query<&mut Sprite, With<Preview>>,
    mut label: Query<&mut Text2d, With<Description>>,
    mut exit: MessageWriter<AppExit>,
) {
    if keys.just_pressed(KeyCode::Escape) {
        exit.write(AppExit::Success);
    }
    let mut changed = false;
    if keys.just_pressed(KeyCode::ArrowRight) {
        gallery.selected = (gallery.selected + 1) % gallery.table.bitmaps.len();
        changed = true;
    }
    if keys.just_pressed(KeyCode::ArrowLeft) {
        gallery.selected =
            (gallery.selected + gallery.table.bitmaps.len() - 1) % gallery.table.bitmaps.len();
        changed = true;
    }
    if keys.just_pressed(KeyCode::KeyA) {
        gallery.alpha_mode = (gallery.alpha_mode + 1) % 3;
        changed = true;
    }
    if changed {
        let (image, size, text) = create_preview(&gallery, &mut images);
        for mut sprite in &mut preview {
            sprite.image = image.clone();
            sprite.custom_size = Some(size);
        }
        for mut value in &mut label {
            value.0 = text.clone();
        }
    }
}

fn load() -> Result<Gallery, Box<dyn Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 2 {
        return Err("Usage: gdl_client TEXDEF.PS2 TEXTURES.PS2".into());
    }
    let mut data = Vec::new();
    for arg in args {
        let p = PathBuf::from(arg);
        if fs::metadata(&p)?.len() > 64 * 1024 * 1024 {
            return Err("source exceeds viewer policy (64 MiB)".into());
        }
        data.push(fs::read(p)?);
    }
    let table = TextureTable::parse(&data[0])?;
    if table.bitmaps.is_empty() {
        return Err("no bitmaps in source".into());
    }
    let texdef_hash = format!("{:x}", Sha256::digest(&data[0]));
    let textures_hash = format!("{:x}", Sha256::digest(&data[1]));
    Ok(Gallery {
        table,
        pixels: data.remove(1),
        selected: 0,
        alpha_mode: 0,
        texdef_hash,
        textures_hash,
    })
}
fn main() {
    let gallery = match load() {
        Ok(value) => value,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    App::new()
        .insert_resource(gallery)
        .insert_resource(ClearColor(Color::srgb(0.08, 0.08, 0.08)))
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "GDL native texture laboratory (M05 research)".into(),
                resolution: (1000, 760).into(),
                ..default()
            }),
            ..default()
        }))
        .add_systems(Startup, setup)
        .add_systems(Update, browse)
        .run();
}

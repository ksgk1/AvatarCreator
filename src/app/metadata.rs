use std::path::PathBuf;
use std::str::FromStr;

use eframe::egui::{Color32, Vec2};
use image::Rgba;
use thiserror::Error;

use super::BorderColours;

/// Keyword of the PNG metadata chunk used to persist the state of a
/// generated avatar.
pub(super) const PNG_METADATA_KEYWORD: &str = "avatar-creator";

/// State persisted in the metadata chunk of a generated avatar.
///
/// All coordinates are in original image pixels. Only the file name of the
/// original image is stored.
pub(super) struct SavedAvatar {
    pub(super) original_file_name: PathBuf,
    pub(super) center: Vec2,
    pub(super) radius: f32,
    pub(super) border_percent: u8,
    pub(super) colours: BorderColours,
}

/// Snapshot of all state the preview depends on, compared every frame to
/// Metadata payload stored in a generated avatar: the file name of the
/// original image on the first line, then `center_x center_y radius
/// border_percent` in original image pixels, then the border colour state,
/// `single r g b`, `gradient angle r1 g1 b1 r2 g2 b2`, or `rainbow angle`.
pub(super) fn selection_metadata_payload(saved: &SavedAvatar) -> String {
    let colour_line = match saved.colours {
        BorderColours::Single(colour) => format!("single {colour}", colour = rgb_text(colour)),
        BorderColours::Gradient {
            angle_deg,
            first,
            last,
        } => format!(
            "gradient {angle} {first} {last}",
            angle = angle_deg,
            first = rgb_text(first),
            last = rgb_text(last),
        ),
        BorderColours::Rainbow { angle_deg } => format!("rainbow {angle_deg}"),
    };
    format!(
        "{file_name}\n{x} {y} {radius} {border_percent}\n{colour_line}",
        file_name = saved.original_file_name.display(),
        x = saved.center.x,
        y = saved.center.y,
        radius = saved.radius,
        border_percent = saved.border_percent,
    )
}

/// Error returned when the metadata payload of a generated avatar cannot be
/// parsed.
#[derive(Debug, Clone, Copy, Error)]
pub(super) enum ParseSelectionError {
    #[error("missing original image path")]
    MissingPath,
    #[error("missing or invalid selection values")]
    InvalidSelection,
    #[error("missing or invalid colour values")]
    InvalidColour,
}

impl FromStr for SavedAvatar {
    type Err = ParseSelectionError;

    fn from_str(payload: &str) -> Result<Self, Self::Err> {
        let mut lines = payload.lines();
        let original_file_name =
            PathBuf::from(lines.next().ok_or(ParseSelectionError::MissingPath)?);
        let mut fields = lines
            .next()
            .ok_or(ParseSelectionError::InvalidSelection)?
            .split_ascii_whitespace();
        let center_x: f32 = parse_field(&mut fields, ParseSelectionError::InvalidSelection)?;
        let center_y: f32 = parse_field(&mut fields, ParseSelectionError::InvalidSelection)?;
        let radius: f32 = parse_field(&mut fields, ParseSelectionError::InvalidSelection)?;
        let border_percent: u8 = parse_field(&mut fields, ParseSelectionError::InvalidSelection)?;
        if !(center_x.is_finite() && center_y.is_finite() && radius.is_finite() && radius >= 0.0) {
            return Err(ParseSelectionError::InvalidSelection);
        }
        let colours = match lines.next() {
            Some(line) => parse_colour_line(line)?,
            None => BorderColours::Single(Color32::WHITE),
        };
        Ok(SavedAvatar {
            original_file_name,
            center: Vec2::new(center_x, center_y),
            radius,
            border_percent,
            colours,
        })
    }
}

/// Parses the colour line of the metadata payload: `single r g b`,
/// `gradient angle r1 g1 b1 r2 g2 b2`, or `rainbow angle`.
fn parse_colour_line(line: &str) -> Result<BorderColours, ParseSelectionError> {
    let mut fields = line.split_ascii_whitespace();
    match fields.next().ok_or(ParseSelectionError::InvalidColour)? {
        "single" => Ok(BorderColours::Single(parse_rgb(&mut fields)?)),
        "gradient" => Ok(BorderColours::Gradient {
            angle_deg: parse_angle(&mut fields)?,
            first: parse_rgb(&mut fields)?,
            last: parse_rgb(&mut fields)?,
        }),
        "rainbow" => Ok(BorderColours::Rainbow {
            angle_deg: parse_angle(&mut fields)?,
        }),
        _ => Err(ParseSelectionError::InvalidColour),
    }
}

/// Parses a finite gradient angle from the metadata payload.
fn parse_angle<'a>(fields: &mut impl Iterator<Item = &'a str>) -> Result<f32, ParseSelectionError> {
    let angle: f32 = parse_field(fields, ParseSelectionError::InvalidColour)?;
    if !angle.is_finite() {
        return Err(ParseSelectionError::InvalidColour);
    }
    Ok(angle)
}

/// Parses the next whitespace-separated field as `T`.
fn parse_field<'a, T: FromStr>(
    fields: &mut impl Iterator<Item = &'a str>,
    error: ParseSelectionError,
) -> Result<T, ParseSelectionError> {
    fields.next().ok_or(error)?.parse().map_err(|_| error)
}

fn parse_rgb<'a>(
    fields: &mut impl Iterator<Item = &'a str>,
) -> Result<Color32, ParseSelectionError> {
    let red: u8 = parse_field(fields, ParseSelectionError::InvalidColour)?;
    let green: u8 = parse_field(fields, ParseSelectionError::InvalidColour)?;
    let blue: u8 = parse_field(fields, ParseSelectionError::InvalidColour)?;
    Ok(Color32::from_rgb(red, green, blue))
}

/// RGB channels of a colour, ignoring alpha.
fn rgb(colour: Color32) -> [u8; 3] {
    let [red, green, blue, _] = colour.to_srgba_unmultiplied();
    [red, green, blue]
}

pub(super) fn opaque_colour(colour: Color32) -> Rgba<u8> {
    let [red, green, blue] = rgb(colour);
    Rgba([red, green, blue, 255])
}

/// Space-separated RGB values of a colour, e.g. `255 0 128`.
fn rgb_text(colour: Color32) -> String {
    let [red, green, blue] = rgb(colour);
    format!("{red} {green} {blue}")
}

/// Rainbow colour at a position between 0 and 1: a fully saturated hue
/// sweep from red through the whole spectrum and back to red.
pub(super) fn rainbow_colour(pos: f32) -> Rgba<u8> {
    let hue = pos.clamp(0.0, 1.0) * 6.0;
    let sector = (hue.floor() as u32) % 6;
    let fraction = hue - hue.floor();
    let (red, green, blue) = match sector {
        0 => (1.0, fraction, 0.0),
        1 => (1.0 - fraction, 1.0, 0.0),
        2 => (0.0, 1.0, fraction),
        3 => (0.0, 1.0 - fraction, 1.0),
        4 => (fraction, 0.0, 1.0),
        _ => (1.0, 0.0, 1.0 - fraction),
    };
    Rgba([
        (red * 255.0).round() as u8,
        (green * 255.0).round() as u8,
        (blue * 255.0).round() as u8,
        255,
    ])
}

/// Linear interpolation between two colours: `pos` 0 returns `first`, 1
/// returns `second`m the result is fully opaque.
#[inline]
pub(super) fn lerp_rgba(first: Rgba<u8>, second: Rgba<u8>, pos: f32) -> Rgba<u8> {
    if first == second {
        return first;
    }
    let pos = pos.clamp(0.0, 1.0);
    let channel = |first: u8, second: u8| {
        let value = (f32::from(first) * (1.0 - pos)) + (f32::from(second) * pos);
        value as u8
    };
    Rgba([
        channel(first[0], second[0]),
        channel(first[1], second[1]),
        channel(first[2], second[2]),
        255,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    use png::text_metadata::ITXtChunk;

    #[test]
    fn lerp_interpolates_in_order() {
        let first = Rgba([0, 0, 0, 255]);
        let second = Rgba([100, 200, 50, 255]);
        assert_eq!(lerp_rgba(first, second, 0.0), first);
        assert_eq!(lerp_rgba(first, second, 1.0), second);
        assert_eq!(lerp_rgba(first, second, 0.5), Rgba([50, 100, 25, 255]));
        // alpha is always opaque, even for transparent inputs
        assert_eq!(lerp_rgba(Rgba([0, 0, 0, 0]), second, 1.0)[3], 255);
    }

    #[test]
    fn rainbow_colour_sweeps_the_spectrum() {
        let red = Rgba([255, 0, 0, 255]);
        assert_eq!(rainbow_colour(0.0), red);
        assert_eq!(rainbow_colour(1.0 / 6.0), Rgba([255, 255, 0, 255]));
        assert_eq!(rainbow_colour(1.0 / 3.0), Rgba([0, 255, 0, 255]));
        assert_eq!(rainbow_colour(0.5), Rgba([0, 255, 255, 255]));
        assert_eq!(rainbow_colour(2.0 / 3.0), Rgba([0, 0, 255, 255]));
        assert_eq!(rainbow_colour(5.0 / 6.0), Rgba([255, 0, 255, 255]));
        // the sweep wraps back to red, and positions are clamped to [0, 1]
        assert_eq!(rainbow_colour(1.0), red);
        assert_eq!(rainbow_colour(-1.0), red);
        assert_eq!(rainbow_colour(2.0), red);
    }

    fn sample_saved_avatar() -> SavedAvatar {
        SavedAvatar {
            original_file_name: PathBuf::from("photo.png"),
            center: Vec2::new(10.5, 20.25),
            radius: 55.0,
            border_percent: 12,
            colours: BorderColours::Single(Color32::from_rgb(158, 27, 50)),
        }
    }

    #[test]
    fn metadata_payload_round_trip() {
        let mut saved = sample_saved_avatar();
        let parsed = selection_metadata_payload(&saved)
            .parse::<SavedAvatar>()
            .unwrap();
        assert_eq!(parsed.original_file_name, PathBuf::from("photo.png"));
        assert_eq!(parsed.center, Vec2::new(10.5, 20.25));
        assert_eq!(parsed.radius, 55.0);
        assert_eq!(parsed.border_percent, 12);
        assert_eq!(
            parsed.colours,
            BorderColours::Single(Color32::from_rgb(158, 27, 50))
        );

        saved.colours = BorderColours::Gradient {
            angle_deg: 45.5,
            first: Color32::from_rgb(10, 20, 30),
            last: Color32::from_rgb(200, 210, 220),
        };
        let parsed = selection_metadata_payload(&saved)
            .parse::<SavedAvatar>()
            .unwrap();
        assert_eq!(parsed.colours, saved.colours);

        saved.colours = BorderColours::Rainbow { angle_deg: 90.0 };
        let parsed = selection_metadata_payload(&saved)
            .parse::<SavedAvatar>()
            .unwrap();
        assert_eq!(parsed.colours, saved.colours);
    }

    #[test]
    fn metadata_payload_rejects_garbage() {
        assert!("".parse::<SavedAvatar>().is_err());
        assert!("only-a-path".parse::<SavedAvatar>().is_err());
        assert!("path\nnot numbers".parse::<SavedAvatar>().is_err());
        assert!("path\n1 2 3 256".parse::<SavedAvatar>().is_err());
        assert!("path\n1 2 -3 4".parse::<SavedAvatar>().is_err());
        assert!("path\n1 2 3 4\nsingle 1 2".parse::<SavedAvatar>().is_err());
        assert!(
            "path\n1 2 3 4\nsingle 1 2 300"
                .parse::<SavedAvatar>()
                .is_err()
        );
        assert!(
            "path\n1 2 3 4\ngradient nan 1 2 3 4 5 6"
                .parse::<SavedAvatar>()
                .is_err()
        );
        assert!(
            "path\n1 2 3 4\nunknown 1 2 3"
                .parse::<SavedAvatar>()
                .is_err()
        );
        assert!("path\n1 2 3 4\nrainbow".parse::<SavedAvatar>().is_err());
        assert!("path\n1 2 3 4\nrainbow nan".parse::<SavedAvatar>().is_err());
    }

    #[test]
    fn metadata_payload_reports_error_kind() {
        assert!(matches!(
            "".parse::<SavedAvatar>(),
            Err(ParseSelectionError::MissingPath)
        ));
        assert!(matches!(
            "path".parse::<SavedAvatar>(),
            Err(ParseSelectionError::InvalidSelection)
        ));
        assert!(matches!(
            "path\n1 2 3 4\nunknown 1 2 3".parse::<SavedAvatar>(),
            Err(ParseSelectionError::InvalidColour)
        ));
    }

    #[test]
    fn metadata_payload_without_colour_line_uses_defaults() {
        let saved: SavedAvatar = "photo.png\n10.5 20.25 55.0 12".parse().unwrap();
        assert_eq!(saved.colours, BorderColours::Single(Color32::WHITE));
    }

    #[test]
    fn png_metadata_chunk_round_trip() {
        let payload = "photo.png\n10.5 20.25 55.0 12\nsingle 158 27 50";
        let mut buffer = Vec::new();
        let mut encoder = png::Encoder::new(&mut buffer, 1, 1);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer
            .write_text_chunk(&ITXtChunk::new(PNG_METADATA_KEYWORD, payload))
            .unwrap();
        writer.write_image_data(&[0, 0, 0, 255]).unwrap();
        drop(writer);

        let reader = png::Decoder::new(std::io::Cursor::new(&buffer))
            .read_info()
            .unwrap();
        let chunk = reader
            .info()
            .utf8_text
            .iter()
            .find(|chunk| chunk.keyword == PNG_METADATA_KEYWORD)
            .unwrap();
        assert_eq!(chunk.get_text().unwrap(), payload);
        let saved = chunk.get_text().unwrap().parse::<SavedAvatar>().unwrap();
        assert_eq!(saved.center, Vec2::new(10.5, 20.25));
        assert_eq!(saved.radius, 55.0);
        assert_eq!(saved.border_percent, 12);
        assert_eq!(
            saved.colours,
            BorderColours::Single(Color32::from_rgb(158, 27, 50))
        );
    }
}

use super::*;

#[test]
fn view_kinds() {
    assert_eq!(view_kind("PLAN.md"), ViewKind::Markdown);
    assert_eq!(view_kind("notes.MARKDOWN"), ViewKind::Markdown);
    assert_eq!(view_kind("package.json"), ViewKind::Json);
    assert_eq!(view_kind("tsconfig.jsonc"), ViewKind::Source);
    assert_eq!(view_kind("main.js"), ViewKind::Source);
    assert_eq!(view_kind(".md"), ViewKind::Source);
    assert_eq!(view_kind("logo.svg"), ViewKind::Svg);
    assert_eq!(view_kind("shot.PNG"), ViewKind::Image);
    assert_eq!(view_kind("photo.avif"), ViewKind::Image);
    assert!(!ViewKind::Source.has_rendered_view());
    assert!(!ViewKind::Image.has_rendered_view());
    assert!(ViewKind::Json.has_rendered_view());
    assert!(ViewKind::Svg.has_rendered_view());
}

#[test]
fn editor_languages() {
    assert_eq!(editor_language("a.json"), "json");
    assert_eq!(editor_language("a.jsonc"), "json");
    assert_eq!(editor_language("app.mjs"), "javascript");
    assert_eq!(editor_language("App.JSX"), "javascript");
    assert_eq!(editor_language("index.ts"), "typescript");
    assert_eq!(editor_language("App.tsx"), "tsx");
    assert_eq!(editor_language("Cargo.toml"), "toml");
    assert_eq!(editor_language("Cargo.lock"), "toml");
    assert_eq!(editor_language("ci.yml"), "yaml");
    assert_eq!(editor_language("main.rs"), "rust");
    assert_eq!(editor_language("run.sh"), "bash");
    assert_eq!(editor_language("README"), "text");
    assert_eq!(editor_language(".bashrc"), "text");
    assert_eq!(editor_language("notes.txt"), "text");
}

#[test]
fn text_is_decoded_unless_binary_or_not_utf8() {
    assert_eq!(decode_text(b"hello\n".to_vec()), Ok("hello\n".into()));
    assert_eq!(
        decode_text(b"\x89PNG\0\0".to_vec()),
        Err(Unviewable::Binary)
    );
    assert_eq!(
        decode_text(vec![0xff, 0xfe, b'a']),
        Err(Unviewable::NotUtf8)
    );
}

#[test]
fn text_and_images_have_their_own_size_limits() {
    assert_eq!(check_size(MAX_VIEW_BYTES), Ok(()));
    let too_large = check_size(MAX_VIEW_BYTES + 1).unwrap_err();
    assert_eq!(
        too_large,
        Unviewable::TooLarge {
            size: MAX_VIEW_BYTES + 1,
            limit: MAX_VIEW_BYTES
        }
    );
    assert_eq!(
        too_large.to_string(),
        "The file is too large to open (2.0 MB, the limit is 2 MB)."
    );

    // An ordinary photo is over the text limit but well within the image one.
    let png = b"\x89PNG\r\n\x1a\n\0\0\0\x0d";
    assert_eq!(check_image_size(4 * 1024 * 1024, png), Ok(()));
    assert_eq!(check_image_size(MAX_IMAGE_BYTES, png), Ok(()));
    let too_large = check_image_size(MAX_IMAGE_BYTES + 1, png).unwrap_err();
    assert_eq!(
        too_large,
        Unviewable::TooLarge {
            size: MAX_IMAGE_BYTES + 1,
            limit: MAX_IMAGE_BYTES
        }
    );
    assert_eq!(
        too_large.to_string(),
        "The file is too large to open (50.0 MB, the limit is 50 MB)."
    );
}

#[test]
fn gif_and_webp_keep_the_text_limit_by_their_bytes() {
    let gif = b"GIF89a\x80\x07\x38\x04\xf7\0";
    let webp = b"RIFF\x10\0\0\0WEBP";
    for header in [&gif[..], &b"GIF87a"[..], &webp[..]] {
        assert_eq!(image_size_limit(header), MAX_VIEW_BYTES);
    }
    assert_eq!(check_image_size(MAX_VIEW_BYTES, gif), Ok(()));
    assert_eq!(
        check_image_size(MAX_VIEW_BYTES + 1, webp),
        Err(Unviewable::TooLarge {
            size: MAX_VIEW_BYTES + 1,
            limit: MAX_VIEW_BYTES
        })
    );

    // Other images, a RIFF file that is not WebP, and a header cut short.
    let jpeg = b"\xff\xd8\xff\xe0\0\x10JFIF\0\x01";
    let wav = b"RIFF\x10\0\0\0WAVE";
    for header in [
        &jpeg[..],
        &wav[..],
        &b"RIFF\x10\0\0\0WE"[..],
        &b"GIF"[..],
        &[][..],
    ] {
        assert_eq!(image_size_limit(header), MAX_IMAGE_BYTES);
    }
}

#[test]
fn images_wider_or_taller_than_a_texture_are_refused() {
    assert_eq!(check_image_dimensions(1, 1), Ok(()));
    assert_eq!(
        check_image_dimensions(MAX_IMAGE_SIDE, MAX_IMAGE_SIDE),
        Ok(())
    );
    assert_eq!(
        check_image_dimensions(MAX_IMAGE_SIDE + 1, 1),
        Err(Unviewable::TooManyPixels {
            width: MAX_IMAGE_SIDE + 1,
            height: 1,
            limit: MAX_IMAGE_SIDE
        })
    );
    let too_tall = check_image_dimensions(4000, 20000).unwrap_err();
    assert_eq!(
        too_tall.to_string(),
        "The image is too large to show (4000 x 20000 pixels, the limit is 16384 on each side)."
    );
}

#[test]
fn json_is_laid_out_in_order_as_written() {
    let text = r#"{"success":{"message":"Hi, \"you\": [1]","status_code":200},
            "b":[1.50,  2e3,true,null], "a": {}, "empty":[ ]}"#;
    let expected = r#"{
  "success": {
    "message": "Hi, \"you\": [1]",
    "status_code": 200
  },
  "b": [
    1.50,
    2e3,
    true,
    null
  ],
  "a": {},
  "empty": []
}"#;
    assert_eq!(pretty_json(text).unwrap(), expected);
}

#[test]
fn json_scalars_and_backslashes_in_strings() {
    assert_eq!(pretty_json(" 42 ").unwrap(), "42");
    assert_eq!(
        pretty_json(r#"["a\\", "b"]"#).unwrap(),
        "[\n  \"a\\\\\",\n  \"b\"\n]"
    );
}

#[test]
fn invalid_json_says_where() {
    let err = pretty_json("[\n  1,\n]").unwrap_err();
    assert_eq!((err.line, err.column), (3, 1));
    assert_eq!(err.message, "expected value");
    assert_eq!(err.to_string(), "Line 3, column 1: expected value");
}

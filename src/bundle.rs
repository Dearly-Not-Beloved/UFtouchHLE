/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Utilities for working with bundles. So far we are only interested in the
//! application bundle.
//!
//! Relevant Apple documentation:
//! * [Bundle Programming Guide](https://developer.apple.com/library/archive/documentation/CoreFoundation/Conceptual/CFBundles/Introduction/Introduction.html)
//!   * [Anatomy of an iOS Application Bundle](https://developer.apple.com/library/archive/documentation/CoreFoundation/Conceptual/CFBundles/BundleTypes/BundleTypes.html)
//! * [Bundle Resources](https://developer.apple.com/documentation/bundleresources?language=objc)

use crate::fs::{BundleData, Fs, GuestPath, GuestPathBuf};
use crate::image::Image;
use crate::window::DeviceFamily;
use plist::dictionary::Dictionary;
use plist::Value;
use std::io::Cursor;

#[derive(Debug)]
pub struct Bundle {
    path: GuestPathBuf,
    plist: Dictionary,
}

impl Bundle {
    /// See [Fs::new] for meaning of `read_only_mode`.
    pub fn new_bundle_and_fs_from_host_path(
        mut bundle_data: BundleData,
        read_only_mode: bool,
    ) -> Result<(Bundle, Fs), String> {
        let plist_bytes = bundle_data.read_plist()?;

        let plist = Value::from_reader(Cursor::new(plist_bytes))
            .map_err(|_| "Could not deserialize plist data".to_string())?;

        let plist = plist
            .into_dictionary()
            .ok_or_else(|| "plist root value is not a dictionary".to_string())?;

        let bundle_name = format!(
            "{}.app",
            if let Some(canonical) = plist.get("CFBundleName") {
                canonical.as_string().unwrap()
            } else {
                bundle_data.bundle_name()
            }
        );
        let bundle_id = plist["CFBundleIdentifier"].as_string().unwrap();

        let (fs, guest_path) = Fs::new(bundle_data, bundle_name, bundle_id, read_only_mode);

        let bundle = Bundle {
            path: guest_path,
            plist,
        };

        Ok((bundle, fs))
    }

    /// Create a fake bundle (see [crate::Environment::new_without_app]).
    pub fn new_fake_bundle() -> Bundle {
        Bundle {
            path: GuestPathBuf::from(String::new()),
            plist: Dictionary::new(),
        }
    }

    pub fn bundle_path(&self) -> &GuestPath {
        &self.path
    }

    pub fn bundle_identifier(&self) -> &str {
        self.plist["CFBundleIdentifier"].as_string().unwrap()
    }

    pub fn bundle_version(&self) -> &str {
        self.plist["CFBundleVersion"].as_string().unwrap()
    }

    pub fn bundle_localizations(&self) -> &[Value] {
        static EMPTY_VAL: Vec<Value> = Vec::new();
        self.plist
            .get("CFBundleLocalizations")
            .and_then(|v| v.as_array())
            .unwrap_or(&EMPTY_VAL)
    }

    /// Canonical name for the bundle according to Info.plist
    pub fn canonical_bundle_name(&self) -> Option<&str> {
        self.plist
            .get("CFBundleName")
            .map(|name| name.as_string().unwrap())
    }

    /// Name for the bundle, either the canonical name or, if there isn't one,
    /// the name this bundle has in the filesystem.
    pub fn bundle_name(&self) -> &str {
        self.path.file_name().unwrap().strip_suffix(".app").unwrap()
    }

    /// The display name as the user would see it on the home screen: the
    /// `CFBundleDisplayName` from the `InfoPlist.strings` of the first of
    /// `languages` (language codes) the app is localized for, or English,
    /// falling back to [Self::display_name].
    pub fn localized_display_name(&self, fs: &Fs, languages: &[String]) -> String {
        use crate::frameworks::foundation::ns_locale::lproj_names_for_language;
        let lprojs = languages
            .iter()
            .map(String::as_str)
            .chain(["en"])
            .flat_map(lproj_names_for_language);
        for lproj in lprojs {
            let path = self.path.join(&lproj).join("InfoPlist.strings");
            let Ok(bytes) = fs.read(&path) else {
                continue;
            };
            if let Some(name) = parse_strings_value(&bytes, "CFBundleDisplayName") {
                // Skip names we couldn't display (e.g. Korean, which the
                // bundled fonts don't cover) in favour of another language.
                if !name.is_empty() && crate::font::bundled_fonts_can_display(&name) {
                    return name;
                }
            }
        }
        self.display_name().to_string()
    }

    pub fn display_name(&self) -> &str {
        if let Some(display_name) = self.plist.get("CFBundleDisplayName") {
            display_name.as_string().unwrap()
        } else {
            ""
        }
    }

    pub fn minimum_os_version(&self) -> Option<&str> {
        self.plist
            .get("MinimumOSVersion")
            .map(|v| v.as_string().unwrap())
    }

    pub fn required_device_capabilities(&self) -> Vec<&str> {
        self.plist
            .get("UIRequiredDeviceCapabilities")
            .map(|v| {
                if let Some(dict) = v.as_dictionary() {
                    // TODO: support undesired capabilities
                    assert!(dict.values().all(|x| x.as_boolean().unwrap()));
                    dict.keys().map(|o| o.as_str()).collect()
                } else {
                    v.as_array()
                        .unwrap()
                        .iter()
                        .map(|o| o.as_string().unwrap())
                        .collect()
                }
            })
            .unwrap_or_default()
    }

    pub fn executable_path(&self) -> GuestPathBuf {
        // FIXME: Is this key optional? All iPhone apps seem to have it.
        self.path
            .join(self.plist["CFBundleExecutable"].as_string().unwrap())
    }

    pub fn launch_image_path(&self) -> GuestPathBuf {
        if let Some(base_name) = self.plist.get("UILaunchImageFile") {
            self.path
                .join(format!("{}.png", base_name.as_string().unwrap()))
        } else {
            self.path.join("Default.png") // not guaranteed to exist!
        }
    }

    pub fn status_bar_hidden(&self) -> bool {
        self.plist
            .get("UIStatusBarHidden")
            .and_then(|v| v.as_boolean())
            .unwrap_or(false)
    }

    fn find_icon(icon_files: &[plist::Value]) -> Option<&plist::Value> {
        ["Icon", "Icon-72"]
            .iter()
            .find_map(|icon| {
                icon_files.iter().find(|found_icon| {
                    found_icon
                        .as_string()
                        .is_some_and(|s| s.trim_end_matches(".png") == *icon)
                })
            })
            .or_else(|| icon_files.first())
    }

    fn icon_path(&self) -> Option<GuestPathBuf> {
        // TODO: Fix this to what an actual iOS device does,
        // including Retina icons and such when we get there.
        // Reference: https://developer.apple.com/library/archive/qa/qa1686/_index.html
        // We check for the icon in the following order:
        // 1. CFBundleIconFile,
        // 2. CFBundleIconFiles for Icon, Icon-72,
        // 3. First in CFBundleIconFiles,
        // (Icon.png / icon.png failsafe is handled in load_icon(),
        // since it needs filesystem access.)
        if let Some(filename) = self.plist.get("CFBundleIconFile").or_else(|| {
            self.plist
                .get("CFBundleIconFiles")
                .and_then(|v| v.as_array())
                .and_then(|a| Self::find_icon(a))
        }) {
            if filename
                .as_string()
                .unwrap()
                .to_lowercase()
                .ends_with(".png")
            {
                Some(self.path.join(filename.as_string().unwrap()))
            } else {
                let filename_with_extension = format!("{}.png", filename.as_string().unwrap());
                Some(self.path.join(filename_with_extension))
            }
        } else {
            None
        }
    }

    /// Load icon and round off its corners (and add sheen if needed) for
    /// display.
    pub fn load_icon(&self, fs: &Fs) -> Result<Image, String> {
        let bytes = if let Some(path) = self.icon_path() {
            fs.read(path)
                .map_err(|_| "Could not read icon file".to_string())?
        } else {
            // No explicit icon was specified in the plist; fall back to
            // checking the filesystem directly for Icon.png, then icon.png.
            fs.read(self.path.join("Icon.png"))
                .or_else(|_| fs.read(self.path.join("icon.png")))
                .map_err(|_| "Could not find app icon (Icon.png or icon.png)".to_string())?
        };
        let mut image =
            Image::from_bytes(&bytes).map_err(|e| format!("Could not parse icon image: {e}"))?;
        // UIPrerenderedIcon is used to avoid iOS applying a sheen effect,
        // should be boolean, but some apps use a string, so we check both.
        // See https://developer.apple.com/library/archive/qa/qa1614/_index.html
        // Default if it does not exist is NO/false.
        let add_sheen = !self
            .plist
            .get("UIPrerenderedIcon")
            .and_then(|v| v.as_boolean().or(v.as_string().map(|s| s == "YES")))
            .unwrap_or(false);
        // iPhone OS icons are 57px by 57px and the OS always applies a
        // 10px radius rounded corner (see e.g. documentation of
        // UIPrerenderedIcon). If the icon is larger for some reason,
        // let's scale to match.
        let corner_radius = (10.0 / 57.0) * (image.dimensions().0 as f32);
        image.round_corners(corner_radius, /* four_corners: */ true, add_sheen);
        Ok(image)
    }

    pub fn main_nib_filename(&self, device_family: Option<DeviceFamily>) -> Option<&str> {
        // TODO: extend this logic for all device-specific keys
        if let Some(device_family) = device_family {
            if device_family == DeviceFamily::iPad && self.plist.get("NSMainNibFile~ipad").is_some()
            {
                return self
                    .plist
                    .get("NSMainNibFile~ipad")
                    .map(|v| v.as_string().unwrap());
            }
        }
        self.plist
            .get("NSMainNibFile")
            .map(|v| v.as_string().unwrap())
    }

    pub fn supported_interface_orientations(&self) -> Vec<&str> {
        // UIInterfaceOrientation (iPhone OS 2.0) is a single string
        // (or a comma separated list of strings).
        // UISupportedInterfaceOrientations (iOS 3.2) is an array of strings and
        // takes precedence.
        self.plist
            .get("UISupportedInterfaceOrientations")
            .map(|v| {
                v.as_array()
                    .unwrap()
                    .iter()
                    .map(|o| o.as_string().unwrap())
                    .collect()
            })
            .unwrap_or_else(|| {
                if let Some(v) = self
                    .plist
                    .get("UIInterfaceOrientation") {
                    let str = v.as_string().unwrap();
                    if str.contains(',') {
                        log!("UIInterfaceOrientation is a comma separated list of strings ({}), splitting!", str);
                    }
                    str.split(',').collect()
                } else {
                    vec!["UIInterfaceOrientationPortrait"]
                }
            })
    }

    pub fn device_family_array(&self) -> Vec<DeviceFamily> {
        self.plist
            .get("UIDeviceFamily")
            .map(|v| {
                v.as_array()
                    .unwrap()
                    .iter()
                    .map(|o| {
                        DeviceFamily::try_from(match o {
                            Value::Integer(i) => i.as_unsigned().unwrap(),
                            Value::String(s) => s.parse().unwrap(),
                            _ => unreachable!(),
                        })
                        .unwrap()
                    })
                    .collect()
            })
            .unwrap_or_else(|| vec![DeviceFamily::iPhone])
    }
}

/// Get a value from a `.strings` file, which is either a property list
/// (binary or XML) or the old text format (`"key" = "value";`).
fn parse_strings_value(bytes: &[u8], key: &str) -> Option<String> {
    if let Ok(Value::Dictionary(dict)) = Value::from_reader(Cursor::new(bytes)) {
        return dict.get(key).and_then(Value::as_string).map(str::to_string);
    }
    // Text format, usually UTF-16 with a byte order mark.
    let text = if bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) {
        let big_endian = bytes[0] == 0xFE;
        let units: Vec<u16> = bytes[2..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&c| {
                if big_endian {
                    u16::from_be_bytes(c)
                } else {
                    u16::from_le_bytes(c)
                }
            })
            .collect();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes)).into_owned()
    };
    for line in text.lines() {
        let line = line.trim();
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        if k.trim().trim_matches('"') != key {
            continue;
        }
        let v = v.trim().trim_end_matches(';').trim();
        let v = v.strip_prefix('"').unwrap_or(v);
        let v = v.strip_suffix('"').unwrap_or(v);
        return Some(v.replace("\\\"", "\""));
    }
    None
}

#[cfg(test)]
#[test]
fn test_parse_strings_value() {
    let text = "/* comment */\n\"CFBundleDisplayName\" = \"My \\\"App\\\"\";\nOther = \"x\";\n";
    assert_eq!(
        parse_strings_value(text.as_bytes(), "CFBundleDisplayName").as_deref(),
        Some("My \"App\"")
    );
    let mut utf16 = vec![0xFF, 0xFE];
    for unit in "CFBundleDisplayName = \"イノティア\";".encode_utf16() {
        utf16.extend_from_slice(&unit.to_le_bytes());
    }
    assert_eq!(
        parse_strings_value(&utf16, "CFBundleDisplayName").as_deref(),
        Some("イノティア")
    );
    assert_eq!(
        parse_strings_value(b"Other = \"x\";", "CFBundleDisplayName"),
        None
    );
}

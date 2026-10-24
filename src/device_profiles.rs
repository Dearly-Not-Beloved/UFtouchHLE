/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Device identities and screen geometry, with app-compatible automatic choice.

use crate::options::Options;
use crate::window::DeviceFamily;

#[derive(Clone, Copy, Debug)]
pub struct DeviceProfile {
    pub machine: &'static str,
    pub name: &'static str,
    pub family: DeviceFamily,
    pub portrait_size: (u32, u32),
    pub retina: bool,
}

const PROFILES: &[DeviceProfile] = &[
    DeviceProfile {
        machine: "iPhone2,1",
        name: "iPhone 3GS",
        family: DeviceFamily::iPhone,
        portrait_size: (320, 480),
        retina: false,
    },
    DeviceProfile {
        machine: "iPhone3,1",
        name: "iPhone 4",
        family: DeviceFamily::iPhone,
        portrait_size: (320, 480),
        retina: true,
    },
    DeviceProfile {
        machine: "iPhone4,1",
        name: "iPhone 4S",
        family: DeviceFamily::iPhone,
        portrait_size: (320, 480),
        retina: true,
    },
    DeviceProfile {
        machine: "iPhone5,1",
        name: "iPhone 5",
        family: DeviceFamily::iPhone,
        portrait_size: (320, 568),
        retina: true,
    },
    DeviceProfile {
        machine: "iPhone6,1",
        name: "iPhone 5s",
        family: DeviceFamily::iPhone,
        portrait_size: (320, 568),
        retina: true,
    },
    DeviceProfile {
        machine: "iPad2,1",
        name: "iPad 2",
        family: DeviceFamily::iPad,
        portrait_size: (768, 1024),
        retina: false,
    },
    DeviceProfile {
        machine: "iPad3,1",
        name: "Retina iPad",
        family: DeviceFamily::iPad,
        portrait_size: (768, 1024),
        retina: true,
    },
];

pub fn profile(machine: &str) -> Option<&'static DeviceProfile> {
    PROFILES.iter().find(|p| p.machine == machine)
}

/// Screen families and the explicit tall-screen opt-in from the app bundle.
#[derive(Clone, Debug)]
pub struct DeviceSupport {
    pub families: Vec<DeviceFamily>,
    pub widescreen: bool,
    pub family_override: Option<DeviceFamily>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResolvedDevice {
    pub machine: &'static str,
    pub family: DeviceFamily,
    pub portrait_size: (u32, u32),
    pub retina: bool,
    pub compatibility_size: bool,
}

impl From<&DeviceProfile> for ResolvedDevice {
    fn from(profile: &DeviceProfile) -> Self {
        Self {
            machine: profile.machine,
            family: profile.family,
            portrait_size: profile.portrait_size,
            retina: profile.retina,
            compatibility_size: false,
        }
    }
}

fn aspect(size: (u32, u32)) -> Option<f64> {
    let short = size.0.min(size.1);
    (short > 0).then(|| size.0.max(size.1) as f64 / short as f64)
}

/// Select screen shape by relative aspect-ratio difference, independent of
/// the host's orientation. Equal shapes preserve the app's existing identity.
pub fn select(
    options: &Options,
    support: &DeviceSupport,
    host_size: Option<(u32, u32)>,
) -> ResolvedDevice {
    if let Some(manual) = profile(options.device_profile) {
        if support.families.contains(&manual.family) {
            let mut result = ResolvedDevice::from(manual);
            if result.family == DeviceFamily::iPhone
                && result.portrait_size.1 > 480
                && !support.widescreen
            {
                // As on an iPhone 5 running an older app, keep its old canvas.
                result.portrait_size = (320, 480);
                result.compatibility_size = true;
            }
            return result;
        }
    }

    let allowed = |family| {
        support.families.contains(&family)
            && support
                .family_override
                .filter(|f| support.families.contains(f))
                .is_none_or(|f| f == family)
    };
    let baseline_family = options.device_family.unwrap_or(DeviceFamily::iPhone);
    let phone_machine = options
        .device_model
        .filter(|m| !m.starts_with("iPad"))
        .unwrap_or("iPhone1,1");
    let tablet_machine = options
        .device_model
        .filter(|m| m.starts_with("iPad"))
        .unwrap_or(if options.retina { "iPad3,1" } else { "iPad2,1" });
    let mut candidates = Vec::new();
    if allowed(DeviceFamily::iPhone) {
        candidates.push(ResolvedDevice {
            machine: phone_machine,
            family: DeviceFamily::iPhone,
            portrait_size: (320, 480),
            retina: options.retina,
            compatibility_size: false,
        });
        if support.widescreen {
            // iPhone 5 is recognized by apps predating the 5s,
            // at the same screen size.
            candidates.push(ResolvedDevice::from(profile("iPhone5,1").unwrap()));
        }
    }
    if allowed(DeviceFamily::iPad) {
        candidates.push(ResolvedDevice {
            machine: tablet_machine,
            family: DeviceFamily::iPad,
            portrait_size: (768, 1024),
            retina: options.retina,
            compatibility_size: false,
        });
    }
    let target = host_size
        .and_then(aspect)
        .unwrap_or_else(|| aspect(baseline_family.portrait_size()).unwrap());
    candidates
        .into_iter()
        .min_by(|a, b| {
            let error = |p: &ResolvedDevice| (aspect(p.portrait_size).unwrap() / target).ln().abs();
            error(a).total_cmp(&error(b))
        })
        .expect("Apps must support at least one device family")
}

/// Apply only resolved display properties; game-specific workarounds and
/// emulated system-version settings are preserved.
pub fn apply(options: &mut Options, support: &DeviceSupport, host_size: Option<(u32, u32)>) {
    let resolved = select(options, support, host_size);
    if options.device_profile != "auto"
        && profile(options.device_profile).is_some_and(|p| !support.families.contains(&p.family))
    {
        echo!("Selected device family is unsupported by this app; using a compatible screen.");
    }
    let name = profile(resolved.machine)
        .map(|p| p.name)
        .unwrap_or(resolved.machine);
    let scale = if resolved.retina { 2 } else { 1 };
    log!(
        "Device: {} ({}), {}x{} points, {}x{} pixels{}.",
        name,
        resolved.machine,
        resolved.portrait_size.0,
        resolved.portrait_size.1,
        resolved.portrait_size.0 * scale,
        resolved.portrait_size.1 * scale,
        if resolved.compatibility_size {
            " (legacy app compatibility size)"
        } else {
            ""
        }
    );
    options.device_model = Some(resolved.machine);
    options.device_family = Some(resolved.family);
    options.retina = resolved.retina;
    options.device_screen_size = Some(resolved.portrait_size);
}

#[cfg(test)]
mod tests {
    use super::*;
    fn support(widescreen: bool) -> DeviceSupport {
        DeviceSupport {
            families: vec![DeviceFamily::iPhone, DeviceFamily::iPad],
            widescreen,
            family_override: None,
        }
    }
    #[test]
    fn auto_matches_phone_and_tablet_shapes_without_stretching() {
        let options = Options::default();
        let supported = support(true);
        for host in [(1080, 2280), (2280, 1080), (1920, 1080)] {
            assert_eq!(
                select(&options, &supported, Some(host)).portrait_size,
                (320, 568)
            );
        }
        assert_eq!(
            select(&options, &supported, Some((2048, 1536))).family,
            DeviceFamily::iPad
        );
        assert_eq!(
            select(&options, &supported, Some((1080, 1620))).portrait_size,
            (320, 480)
        );
    }
    #[test]
    fn old_phone_apps_keep_compatible_size_and_pinned_identity() {
        let mut options = Options {
            device_model: Some("iPhone3,1"),
            ..Options::default()
        };
        let mut supported = support(false);
        supported.families = vec![DeviceFamily::iPhone];
        let chosen = select(&options, &supported, Some((1080, 2400)));
        assert_eq!(chosen.machine, "iPhone3,1");
        assert_eq!(chosen.portrait_size, (320, 480));
        options.device_profile = "iPhone6,1";
        let chosen = select(&options, &supported, Some((1080, 2400)));
        assert_eq!(chosen.machine, "iPhone6,1");
        assert!(chosen.compatibility_size);
        assert_eq!(chosen.portrait_size, (320, 480));
    }
    #[test]
    fn manual_choice_controls_identity_retina_and_geometry() {
        let mut options = Options {
            device_profile: "iPhone6,1",
            ..Options::default()
        };
        let chosen = select(&options, &support(true), Some((1536, 2048)));
        assert_eq!(chosen.machine, "iPhone6,1");
        assert_eq!(chosen.portrait_size, (320, 568));
        assert!(chosen.retina);
        options.device_profile = "iPad3,1";
        let mut supported = support(true);
        supported.families = vec![DeviceFamily::iPhone];
        assert_eq!(
            select(&options, &supported, Some((1080, 2400))).family,
            DeviceFamily::iPhone
        );
    }
    #[test]
    fn family_overrides_and_headless_have_deterministic_fallbacks() {
        let options = Options::default();
        let mut supported = support(true);
        supported.family_override = Some(DeviceFamily::iPad);
        assert_eq!(
            select(&options, &supported, Some((1080, 2400))).family,
            DeviceFamily::iPad
        );
        assert_eq!(
            select(&options, &support(true), None).portrait_size,
            (320, 480)
        );
        assert_eq!(
            select(&options, &support(true), Some((0, 0))).portrait_size,
            (320, 480)
        );
    }
    #[test]
    fn apply_preserves_non_display_options() {
        let mut options = Options {
            device_profile: "iPhone6,1",
            ..Options::default()
        };
        apply(&mut options, &support(true), Some((1080, 2400)));
        assert_eq!(options.device_model, Some("iPhone6,1"));
        assert_eq!(options.device_screen_size, Some((320, 568)));
    }
}

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
    pub scale: u32,
}

const PROFILES: &[DeviceProfile] = &[
    DeviceProfile {
        machine: "iPhone2,1",
        name: "iPhone 3GS",
        family: DeviceFamily::iPhone,
        portrait_size: (320, 480),
        scale: 1,
    },
    DeviceProfile {
        machine: "iPhone3,1",
        name: "iPhone 4",
        family: DeviceFamily::iPhone,
        portrait_size: (320, 480),
        scale: 2,
    },
    DeviceProfile {
        machine: "iPhone4,1",
        name: "iPhone 4S",
        family: DeviceFamily::iPhone,
        portrait_size: (320, 480),
        scale: 2,
    },
    DeviceProfile {
        machine: "iPhone5,1",
        name: "iPhone 5",
        family: DeviceFamily::iPhone,
        portrait_size: (320, 568),
        scale: 2,
    },
    DeviceProfile {
        machine: "iPhone6,1",
        name: "iPhone 5s",
        family: DeviceFamily::iPhone,
        portrait_size: (320, 568),
        scale: 2,
    },
    DeviceProfile {
        machine: "iPhone7,2",
        name: "iPhone 6",
        family: DeviceFamily::iPhone,
        portrait_size: (375, 667),
        scale: 2,
    },
    DeviceProfile {
        machine: "iPhone7,1",
        name: "iPhone 6 Plus",
        family: DeviceFamily::iPhone,
        portrait_size: (414, 736),
        scale: 3,
    },
    DeviceProfile {
        machine: "iPad2,1",
        name: "iPad 2",
        family: DeviceFamily::iPad,
        portrait_size: (768, 1024),
        scale: 1,
    },
    DeviceProfile {
        machine: "iPad3,1",
        name: "Retina iPad",
        family: DeviceFamily::iPad,
        portrait_size: (768, 1024),
        scale: 2,
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
    /// Explicit large-phone point sizes from launch resources/declarations.
    pub large_phone_sizes: Vec<(u32, u32)>,
    pub family_override: Option<DeviceFamily>,
    /// Whether the app has Retina (@2x) images, so it can use a Retina
    /// screen rather than being scaled up.
    pub retina_assets: bool,
}

/// What's known about the host's screen. The physical size is only given
/// on Android, where it tells phones from tablets.
#[derive(Clone, Copy, Debug)]
pub struct HostScreen {
    /// Pixels.
    pub size: (u32, u32),
    /// Diagonal, in inches.
    pub inches: Option<f32>,
}

/// Hosts at least this large (diagonally) are tablets.
const TABLET_INCHES: f32 = 7.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResolvedDevice {
    pub machine: &'static str,
    pub family: DeviceFamily,
    pub portrait_size: (u32, u32),
    pub scale: u32,
    pub compatibility_size: bool,
}

impl From<&DeviceProfile> for ResolvedDevice {
    fn from(profile: &DeviceProfile) -> Self {
        Self {
            machine: profile.machine,
            family: profile.family,
            portrait_size: profile.portrait_size,
            scale: profile.scale,
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
///
/// Apps with Retina images get a Retina model if the host has the pixels for
/// it. When the host's physical size is known, phones get an iPhone and
/// tablets an iPad (if the app supports it).
pub fn select(
    options: &Options,
    support: &DeviceSupport,
    host: Option<HostScreen>,
) -> ResolvedDevice {
    let host_size = host.map(|h| h.size);
    if let Some(manual) = profile(options.device_profile) {
        if support.families.contains(&manual.family) {
            let mut result = ResolvedDevice::from(manual);
            if result.family == DeviceFamily::iPhone {
                if result.portrait_size.0 > 320
                    && !support.large_phone_sizes.contains(&result.portrait_size)
                {
                    result.portrait_size = if support.widescreen {
                        (320, 568)
                    } else {
                        (320, 480)
                    };
                    // iOS displays older phone apps in a 2x-compatible canvas,
                    // including on Plus hardware with a 3x native profile.
                    result.scale = 2;
                    result.compatibility_size = true;
                } else if result.portrait_size.1 > 480 && !support.widescreen {
                    result.portrait_size = (320, 480);
                    result.scale = 2;
                    result.compatibility_size = true;
                }
            }
            return result;
        }
    }

    let supported = |family| {
        support.families.contains(&family)
            && support
                .family_override
                .filter(|f| support.families.contains(f))
                .is_none_or(|f| f == family)
    };
    // A phone or tablet only considers its own kind of device, if the app
    // supports it: screen shape alone can't tell a 7:5 tablet from a phone.
    let host_family = host.and_then(|h| h.inches).map(|inches| {
        if inches >= TABLET_INCHES {
            DeviceFamily::iPad
        } else {
            DeviceFamily::iPhone
        }
    });
    let allowed =
        |family| supported(family) && host_family.is_none_or(|f| f == family || !supported(f));
    // Retina if the app has the images for it and the host has at least 1.5
    // pixels for each of a Retina device's.
    let auto_retina = |points: (u32, u32)| {
        support.retina_assets
            && host_size.is_some_and(|(w, h)| w.min(h) * 2 >= points.0.min(points.1) * 3)
    };
    let phone_retina = options.retina || auto_retina((320, 480));
    let tablet_retina = options.retina || auto_retina((768, 1024));
    let baseline_family = options.device_family.unwrap_or(DeviceFamily::iPhone);
    // Apps may only recognise a Retina screen on a model that has one.
    let phone_machine = options
        .device_model
        .filter(|m| !m.starts_with("iPad"))
        .unwrap_or(if phone_retina && !options.retina {
            "iPhone4,1"
        } else {
            "iPhone1,1"
        });
    let tablet_machine = options
        .device_model
        .filter(|m| m.starts_with("iPad"))
        .unwrap_or(if tablet_retina { "iPad3,1" } else { "iPad2,1" });
    let mut candidates = Vec::new();
    if allowed(DeviceFamily::iPhone) {
        candidates.push(ResolvedDevice {
            machine: phone_machine,
            family: DeviceFamily::iPhone,
            portrait_size: (320, 480),
            scale: if phone_retina { 2 } else { 1 },
            compatibility_size: false,
        });
        if support.widescreen {
            // iPhone 5 is recognized by apps predating the 5s,
            // at the same screen size.
            candidates.push(ResolvedDevice::from(profile("iPhone5,1").unwrap()));
        }
    }
    if allowed(DeviceFamily::iPhone) {
        for machine in ["iPhone7,2", "iPhone7,1"] {
            let candidate = profile(machine).unwrap();
            if support.large_phone_sizes.contains(&candidate.portrait_size) {
                candidates.push(ResolvedDevice::from(candidate));
            }
        }
    }
    if allowed(DeviceFamily::iPad) {
        candidates.push(ResolvedDevice {
            machine: tablet_machine,
            family: DeviceFamily::iPad,
            portrait_size: (768, 1024),
            scale: if tablet_retina { 2 } else { 1 },
            compatibility_size: false,
        });
    }
    let target = host_size
        .and_then(aspect)
        .unwrap_or_else(|| aspect(baseline_family.portrait_size()).unwrap());
    // The rounded 16:9 point sizes differ by less than 0.5%. Prefer the
    // existing lower-resolution candidate for effectively equal screen shapes.
    let mut best: Option<(ResolvedDevice, f64)> = None;
    for candidate in candidates {
        let error = (aspect(candidate.portrait_size).unwrap() / target)
            .ln()
            .abs();
        if best
            .as_ref()
            .is_none_or(|(_, previous)| error < *previous - 0.005)
        {
            best = Some((candidate, error));
        }
    }
    best.expect("Apps must support at least one device family")
        .0
}

/// Apply only resolved display properties; game-specific workarounds and
/// emulated system-version settings are preserved.
pub fn apply(options: &mut Options, support: &DeviceSupport, host: Option<HostScreen>) {
    let resolved = select(options, support, host);
    if options.device_profile != "auto"
        && profile(options.device_profile).is_some_and(|p| !support.families.contains(&p.family))
    {
        echo!("Selected device family is unsupported by this app; using a compatible screen.");
    }
    options.device_model = Some(resolved.machine);
    options.device_family = Some(resolved.family);
    options.retina = resolved.scale > 1;
    options.device_screen_scale = Some(resolved.scale);
    options.device_screen_size = Some(resolved.portrait_size);
    crate::performance::configure(options);
    let name = profile(resolved.machine)
        .map(|p| p.name)
        .unwrap_or(resolved.machine);
    let scale = options.screen_scale();
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
}

#[cfg(test)]
mod tests {
    use super::*;
    pub(super) fn screen(size: (u32, u32)) -> HostScreen {
        HostScreen { size, inches: None }
    }
    fn support(widescreen: bool) -> DeviceSupport {
        DeviceSupport {
            families: vec![DeviceFamily::iPhone, DeviceFamily::iPad],
            widescreen,
            large_phone_sizes: Vec::new(),
            family_override: None,
            retina_assets: false,
        }
    }
    #[test]
    fn auto_matches_phone_and_tablet_shapes_without_stretching() {
        let options = Options::default();
        let supported = support(true);
        for host in [(1080, 2280), (2280, 1080), (1920, 1080)] {
            assert_eq!(
                select(&options, &supported, Some(screen(host))).portrait_size,
                (320, 568)
            );
        }
        assert_eq!(
            select(&options, &supported, Some(screen((2048, 1536)))).family,
            DeviceFamily::iPad
        );
        assert_eq!(
            select(&options, &supported, Some(screen((1080, 1620)))).portrait_size,
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
        let chosen = select(&options, &supported, Some(screen((1080, 2400))));
        assert_eq!(chosen.machine, "iPhone3,1");
        assert_eq!(chosen.portrait_size, (320, 480));
        options.device_profile = "iPhone6,1";
        let chosen = select(&options, &supported, Some(screen((1080, 2400))));
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
        let chosen = select(&options, &support(true), Some(screen((1536, 2048))));
        assert_eq!(chosen.machine, "iPhone6,1");
        assert_eq!(chosen.portrait_size, (320, 568));
        assert_eq!(chosen.scale, 2);
        options.device_profile = "iPad3,1";
        let mut supported = support(true);
        supported.families = vec![DeviceFamily::iPhone];
        assert_eq!(
            select(&options, &supported, Some(screen((1080, 2400)))).family,
            DeviceFamily::iPhone
        );
    }
    #[test]
    fn family_overrides_and_headless_have_deterministic_fallbacks() {
        let options = Options::default();
        let mut supported = support(true);
        supported.family_override = Some(DeviceFamily::iPad);
        assert_eq!(
            select(&options, &supported, Some(screen((1080, 2400)))).family,
            DeviceFamily::iPad
        );
        assert_eq!(
            select(&options, &support(true), None).portrait_size,
            (320, 480)
        );
        assert_eq!(
            select(&options, &support(true), Some(screen((0, 0)))).portrait_size,
            (320, 480)
        );
    }
    #[test]
    fn android_hosts_pick_their_own_family_and_retina() {
        let options = Options::default();
        let mut supported = support(false);
        supported.retina_assets = true;
        // A 7:5 tablet is about as close to 3:2 as to 4:3; being a tablet
        // decides it.
        let tablet = HostScreen {
            size: (2400, 3392),
            inches: Some(9.9),
        };
        let chosen = select(&options, &supported, Some(tablet));
        assert_eq!(chosen.machine, "iPad3,1");
        assert_eq!(chosen.scale, 2);
        let phone = HostScreen {
            size: (1080, 2340),
            inches: Some(6.4),
        };
        let chosen = select(&options, &supported, Some(phone));
        assert_eq!((chosen.machine, chosen.scale), ("iPhone4,1", 2));
        // Without Retina images, or the pixels for them, apps stay at 1x.
        supported.retina_assets = false;
        assert_eq!(select(&options, &supported, Some(phone)).scale, 1);
        supported.retina_assets = true;
        let small_tablet = HostScreen {
            size: (800, 1280),
            inches: Some(8.0),
        };
        let chosen = select(&options, &supported, Some(small_tablet));
        assert_eq!((chosen.machine, chosen.scale), ("iPad2,1", 1));
        // Phone-only apps still run on tablets.
        supported.families = vec![DeviceFamily::iPhone];
        assert_eq!(
            select(&options, &supported, Some(tablet)).family,
            DeviceFamily::iPhone
        );
    }
    #[test]
    fn apply_preserves_non_display_options() {
        let mut options = Options {
            device_profile: "iPhone6,1",
            system_version: "7.0".into(),
            ..Options::default()
        };
        apply(&mut options, &support(true), Some(screen((1080, 2400))));
        assert_eq!(options.system_version, "7.0");
        assert_eq!(options.device_model, Some("iPhone6,1"));
        assert_eq!(options.device_screen_size, Some((320, 568)));
    }
}

#[cfg(test)]
mod large_phone_tests {
    use super::tests::screen;
    use super::*;

    fn support(large_phone_sizes: Vec<(u32, u32)>) -> DeviceSupport {
        DeviceSupport {
            families: vec![DeviceFamily::iPhone],
            widescreen: true,
            large_phone_sizes,
            family_override: None,
            retina_assets: false,
        }
    }

    #[test]
    fn six_and_plus_resolve_native_points_and_pixel_multipliers() {
        let supported = support(vec![(375, 667), (414, 736)]);
        for (machine, points, scale, pixels) in [
            ("iPhone7,2", (375, 667), 2, (750, 1334)),
            ("iPhone7,1", (414, 736), 3, (1242, 2208)),
        ] {
            let mut options = Options {
                device_profile: machine,
                ..Options::default()
            };
            apply(&mut options, &supported, Some(screen((1080, 2280))));
            assert_eq!(options.device_screen_size, Some(points));
            assert_eq!(options.screen_scale(), scale);
            assert_eq!((points.0 * scale, points.1 * scale), pixels);
            assert_eq!(options.device_model, Some(machine));
            assert!(options.retina);
        }
    }

    #[test]
    fn older_games_use_two_x_compatibility_canvas_on_plus() {
        let options = Options {
            device_profile: "iPhone7,1",
            ..Options::default()
        };
        let mut supported = support(Vec::new());
        let chosen = select(&options, &supported, None);
        assert_eq!(chosen.machine, "iPhone7,1");
        assert_eq!(chosen.portrait_size, (320, 568));
        assert_eq!(chosen.scale, 2);
        assert!(chosen.compatibility_size);
        supported.widescreen = false;
        assert_eq!(select(&options, &supported, None).portrait_size, (320, 480));
    }

    #[test]
    fn auto_keeps_efficient_profile_for_equivalent_sixteen_nine_shapes() {
        let supported = support(vec![(375, 667), (414, 736)]);
        let options = Options::default();
        for host in [(1080, 1920), (1080, 2280), (1334, 750)] {
            assert_eq!(
                select(&options, &supported, Some(screen(host))).machine,
                "iPhone5,1"
            );
        }
    }
}

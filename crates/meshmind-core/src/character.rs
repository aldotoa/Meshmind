//! Character starter bodies: a humanoid built from named, blended volumes
//! with believable proportions for a few common species.
//!
//! The result is a list of [`SdfPart`]s (so every part can be adjusted and
//! the body rebuilt) plus landmarks: where the eyes, hands, belt and so on
//! are, so clothing, armour, weapons and eyes can be placed precisely.
//!
//! Conventions: feet on y = 0, facing +Z, the character's left on +X,
//! metres. Proportions are measured in head heights, as artists do.

use std::collections::BTreeMap;

use glam::{EulerRot, Mat3, Quat, Vec3};
use serde::{Deserialize, Serialize};

use crate::sdf::{capsule, ellipsoid, rounded_box, SdfPart};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Species {
    Human,
    Orc,
    Goblin,
    Dwarf,
    Troll,
}

impl Species {
    pub const ALL: [Species; 5] = [
        Species::Human,
        Species::Orc,
        Species::Goblin,
        Species::Dwarf,
        Species::Troll,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Species::Human => "human",
            Species::Orc => "orc",
            Species::Goblin => "goblin",
            Species::Dwarf => "dwarf",
            Species::Troll => "troll",
        }
    }
    pub fn from_name(s: &str) -> Option<Species> {
        Species::ALL.into_iter().find(|p| p.name() == s)
    }
    /// A typical standing height in metres.
    pub fn default_height(self) -> f32 {
        match self {
            Species::Human => 1.8,
            Species::Orc => 2.05,
            Species::Goblin => 1.0,
            Species::Dwarf => 1.35,
            Species::Troll => 2.8,
        }
    }
    /// A typical skin colour (sRGB hex) for the agent to start from.
    pub fn skin_hex(self) -> &'static str {
        match self {
            Species::Human => "#c58c6a",
            Species::Orc => "#3d5226",
            Species::Goblin => "#5c6b2c",
            Species::Dwarf => "#c98f72",
            Species::Troll => "#4c5759",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ears {
    Round,
    Pointed,
    Long,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Pose {
    /// Arms about 45 degrees from the body: the standard pose for rigging.
    APose,
    /// Arms hanging near the body: reads better in stills.
    Relaxed,
}

/// Everything that shapes a starter body. Unset options use the species' own.
#[derive(Debug, Clone, Default)]
pub struct CharacterParams {
    pub species: Option<Species>,
    pub height: Option<f32>,
    pub feminine: bool,
    /// 0 lean, 0.5 average, 1 massive.
    pub bulk: Option<f32>,
    /// 0 flat stomach, 1 big belly.
    pub gut: Option<f32>,
    /// Multiplier on head size.
    pub head_size: Option<f32>,
    pub arm_length: Option<f32>,
    pub leg_length: Option<f32>,
    /// 0 upright, 1 strongly hunched.
    pub hunch: Option<f32>,
    /// 0 small jaw, 1 heavy jaw with an underbite.
    pub jaw: Option<f32>,
    pub ears: Option<Ears>,
    pub pose: Option<Pose>,
}

/// Species proportions. Lengths ending in `_h` are fractions of total height;
/// `hh` values are fractions of a head height.
struct Spec {
    heads: f32,
    leg_h: f32,
    shoulder_h: f32,
    limb: f32,
    chest: f32,
    bulk: f32,
    gut: f32,
    hunch: f32,
    jaw: f32,
    brow: f32,
    ears: Ears,
    arm_h: f32,
    neck_hh: f32,
    head_w: f32,
    nose: Nose,
    hands: f32,
    /// Waist width relative to the shoulders.
    waist: f32,
}

#[derive(Clone, Copy, PartialEq)]
enum Nose {
    Human,
    Flat,
    Long,
    Big,
}

fn spec(s: Species) -> Spec {
    match s {
        Species::Human => Spec {
            heads: 7.5,
            leg_h: 0.47,
            shoulder_h: 0.1,
            limb: 1.0,
            chest: 1.0,
            bulk: 0.45,
            gut: 0.0,
            hunch: 0.0,
            jaw: 0.35,
            brow: 1.0,
            ears: Ears::Round,
            arm_h: 0.44,
            neck_hh: 0.42,
            head_w: 1.0,
            nose: Nose::Human,
            hands: 1.0,
            waist: 0.8,
        },
        Species::Orc => Spec {
            heads: 6.6,
            leg_h: 0.44,
            shoulder_h: 0.128,
            limb: 1.4,
            chest: 1.5,
            bulk: 0.75,
            gut: 0.35,
            hunch: 0.35,
            jaw: 0.85,
            brow: 1.7,
            ears: Ears::Pointed,
            arm_h: 0.47,
            neck_hh: 0.25,
            head_w: 1.08,
            nose: Nose::Flat,
            hands: 1.2,
            waist: 0.66,
        },
        Species::Goblin => Spec {
            heads: 4.3,
            leg_h: 0.37,
            shoulder_h: 0.1,
            limb: 0.8,
            chest: 0.85,
            bulk: 0.2,
            gut: 0.6,
            hunch: 0.55,
            jaw: 0.5,
            brow: 1.2,
            ears: Ears::Long,
            arm_h: 0.48,
            neck_hh: 0.2,
            head_w: 1.0,
            nose: Nose::Long,
            hands: 1.15,
            waist: 0.8,
        },
        Species::Dwarf => Spec {
            heads: 4.9,
            leg_h: 0.34,
            shoulder_h: 0.145,
            limb: 1.55,
            chest: 1.4,
            bulk: 0.8,
            gut: 0.5,
            hunch: 0.1,
            jaw: 0.5,
            brow: 1.4,
            ears: Ears::Round,
            arm_h: 0.4,
            neck_hh: 0.18,
            head_w: 1.05,
            nose: Nose::Big,
            hands: 1.25,
            waist: 0.85,
        },
        Species::Troll => Spec {
            heads: 7.4,
            leg_h: 0.4,
            shoulder_h: 0.135,
            limb: 1.6,
            chest: 1.4,
            bulk: 0.85,
            gut: 0.5,
            hunch: 0.7,
            jaw: 1.0,
            brow: 2.0,
            ears: Ears::Pointed,
            arm_h: 0.53,
            neck_hh: 0.2,
            head_w: 1.0,
            nose: Nose::Long,
            hands: 1.35,
            waist: 0.7,
        },
    }
}

/// A starter body: its parts and named points on it (world space).
pub struct Character {
    pub parts: Vec<SdfPart>,
    pub landmarks: BTreeMap<String, Vec3>,
    pub eye_radius: f32,
    pub species: Species,
    pub height: f32,
}

/// Rotation (Euler XYZ degrees, as parts use) that maps local axes to the
/// given world axes.
fn euler_from_axes(x: Vec3, y: Vec3, z: Vec3) -> Vec3 {
    let q = Quat::from_mat3(&Mat3::from_cols(x, y, z));
    let (a, b, c) = q.to_euler(EulerRot::XYZ);
    Vec3::new(a.to_degrees(), b.to_degrees(), c.to_degrees())
}

fn rot_about(v: Vec3, axis: Vec3, degrees: f32) -> Vec3 {
    Quat::from_axis_angle(axis.normalize(), degrees.to_radians()) * v
}

pub fn build_character(p: &CharacterParams) -> Result<Character, String> {
    let species = p.species.unwrap_or(Species::Human);
    let sp = spec(species);
    let h = p.height.unwrap_or(species.default_height());
    if !(0.1..=50.0).contains(&h) {
        return Err("`height` must be between 0.1 and 50 metres".into());
    }
    let clamp01 = |v: Option<f32>, d: f32| v.unwrap_or(d).clamp(0.0, 1.0);
    let bulk = clamp01(p.bulk, sp.bulk);
    let gut = clamp01(p.gut, sp.gut);
    let hunch = clamp01(p.hunch, sp.hunch);
    let jaw = clamp01(p.jaw, sp.jaw);
    let head_size = p.head_size.unwrap_or(1.0).clamp(0.5, 2.0);
    let arm_len = p.arm_length.unwrap_or(1.0).clamp(0.6, 1.6);
    let leg_len = p.leg_length.unwrap_or(1.0).clamp(0.6, 1.5);
    let ears = p.ears.unwrap_or(sp.ears);
    let pose = p.pose.unwrap_or(Pose::Relaxed);
    let fem = if p.feminine { 1.0 } else { 0.0 };

    // Bulk widens the torso and thickens limbs around the species' build.
    let mass = 0.75 + bulk * 0.55;
    let limb = sp.limb * (0.8 + bulk * 0.45) * (1.0 - fem * 0.12);
    let hh = h / sp.heads; // body proportions use the species' head height
    let head_hh = hh * head_size;

    let top = h;
    let chin = top - head_hh;
    let neck_base = chin - sp.neck_hh * hh;
    let shoulder_y = neck_base - 0.12 * hh;
    let crotch = (sp.leg_h * leg_len * h).min(shoulder_y - 0.25 * h);
    let knee_y = crotch * 0.54;
    let ankle_y = crotch * 0.09;
    let torso = shoulder_y - crotch;
    let sx = sp.shoulder_h * h * mass.sqrt() * (1.0 - fem * 0.14);
    let lean = hunch * 0.09 * torso; // how far the chest leans forward
    let headz = hunch * 0.35 * head_hh + lean;
    let headdrop = hunch * 0.35 * hh;

    let mut parts: Vec<SdfPart> = Vec::new();
    let mut lm: BTreeMap<String, Vec3> = BTreeMap::new();
    let b = |k: f32| k * h; // blend sizes scale with height
    let v = Vec3::new;

    // ---- Torso -------------------------------------------------------------
    let pelvis_c = v(0.0, crotch + 0.07 * torso, -0.01 * h);
    let hip_w = sx * (0.74 + fem * 0.3) * mass.sqrt();
    parts.push(SdfPart::add(
        "pelvis",
        ellipsoid(pelvis_c, v(hip_w, 0.17 * torso, 0.6 * sx * mass)),
        b(0.0),
    ));
    let belly_c = v(0.0, crotch + 0.36 * torso, 0.005 * h + lean * 0.4);
    parts.push(SdfPart::add(
        "abdomen",
        ellipsoid(
            belly_c,
            v(
                sx * (sp.waist - fem * 0.12) * mass.sqrt(),
                0.32 * torso,
                0.55 * sx * mass,
            ),
        ),
        b(0.05),
    ));
    let rib_c = v(0.0, shoulder_y - 0.3 * torso, 0.005 * h + lean);
    parts.push(SdfPart::add(
        "ribcage",
        ellipsoid(
            rib_c,
            v(
                sx * 0.86 * sp.chest.sqrt(),
                0.34 * torso,
                0.62 * sx * sp.chest.sqrt(),
            ),
        ),
        b(0.035),
    ));
    if gut > 0.0 {
        let g = gut * mass;
        parts.push(SdfPart::add(
            "belly",
            ellipsoid(
                v(
                    0.0,
                    crotch + 0.3 * torso,
                    0.12 * sx + lean * 0.5 + 0.18 * sx * g,
                ),
                v(
                    sx * (0.5 + 0.25 * g),
                    0.26 * torso * (0.8 + 0.4 * g),
                    sx * (0.45 + 0.2 * g),
                ),
            ),
            b(0.05),
        ));
    }
    // Chest muscles (or breasts) and the upper back.
    let pec_y = shoulder_y - 0.2 * torso;
    if fem > 0.0 {
        parts.push(
            SdfPart::add(
                "breast",
                ellipsoid(
                    v(0.36 * sx, pec_y - 0.06 * torso, 0.5 * sx + lean),
                    v(0.3 * sx, 0.28 * sx, 0.26 * sx),
                ),
                b(0.03),
            )
            .mirrored(),
        );
    } else {
        parts.push(
            SdfPart::add(
                "chest muscle",
                ellipsoid(
                    v(0.36 * sx, pec_y, 0.36 * sx * sp.chest.sqrt() + lean),
                    v(0.4 * sx, 0.15 * torso, 0.24 * sx * sp.chest.sqrt()),
                ),
                b(0.045),
            )
            .mirrored()
            .rotated(v(0.0, 0.0, -12.0)),
        );
    }
    parts.push(
        SdfPart::add(
            "back muscle",
            ellipsoid(
                v(
                    0.45 * sx,
                    shoulder_y - 0.33 * torso,
                    -0.22 * sx + lean * 0.6,
                ),
                v(
                    0.35 * sx * mass.sqrt() * sp.chest.sqrt(),
                    0.32 * torso,
                    0.3 * sx,
                ),
            ),
            b(0.03),
        )
        .mirrored(),
    );
    if hunch > 0.2 {
        parts.push(SdfPart::add(
            "upper back hump",
            ellipsoid(
                v(0.0, shoulder_y - 0.05 * torso, -0.3 * sx + lean * 0.5),
                v(0.55 * sx, 0.22 * torso, 0.35 * sx * (0.7 + hunch)),
            ),
            b(0.04),
        ));
    }
    parts.push(
        SdfPart::add(
            "buttock",
            ellipsoid(
                v(0.42 * hip_w, crotch + 0.04 * torso, -0.3 * sx * mass.sqrt()),
                v(0.46 * hip_w, 0.16 * torso, 0.3 * sx * mass.sqrt()),
            ),
            b(0.03),
        )
        .mirrored(),
    );

    // ---- Neck and shoulders -----------------------------------------------
    let neck_r = 0.27 * hh * (0.75 + 0.5 * bulk) * (sp.limb / 1.2).max(0.85) * (1.0 - fem * 0.15);
    let neck_top = v(
        0.0,
        chin + 0.25 * head_hh - headdrop,
        headz - 0.08 * head_hh,
    );
    let neck_bot = v(0.0, neck_base - 0.1 * hh, lean * 0.7 - 0.05 * hh);
    parts.push(SdfPart::add(
        "neck",
        capsule(neck_bot, neck_top, neck_r * 1.15, neck_r),
        b(0.03),
    ));
    let sj = v(sx, shoulder_y - 0.05 * hh, lean * 0.6);
    parts.push(
        SdfPart::add(
            "trapezius",
            capsule(
                v(0.0, neck_base + 0.12 * hh, neck_bot.z - 0.03 * hh),
                sj + v(-0.15 * sx, 0.05 * hh, -0.05 * hh),
                neck_r * 0.75,
                neck_r * 0.55,
            ),
            b(0.045),
        )
        .mirrored(),
    );
    let delt_r = 0.023 * h * limb;
    let spread_pre = match pose {
        Pose::APose => 45.0f32,
        Pose::Relaxed => 10.0 + bulk * 8.0,
    };
    let delt_dir = Vec3::new(
        spread_pre.to_radians().sin(),
        -spread_pre.to_radians().cos(),
        0.0,
    );
    parts.push(
        SdfPart::add(
            "shoulder muscle",
            ellipsoid(
                sj + delt_dir * delt_r * 0.9 + v(0.004 * h, 0.0, 0.0),
                v(delt_r * 1.0, delt_r * 1.45, delt_r * 1.1),
            ),
            b(0.03),
        )
        .mirrored()
        .rotated(v(0.0, 0.0, spread_pre)),
    );

    // ---- Arms and hands ----------------------------------------------------
    let arm_total = sp.arm_h * h * arm_len;
    let (upper_l, fore_l, hand_l) = (
        arm_total * 0.43,
        arm_total * 0.33,
        arm_total * 0.24 * sp.hands.sqrt(),
    );
    let spread = match pose {
        Pose::APose => 45.0f32,
        Pose::Relaxed => 10.0 + bulk * 8.0,
    };
    let down = v(spread.to_radians().sin(), -spread.to_radians().cos(), 0.0);
    let elbow = sj + down * upper_l + v(0.0, 0.0, -0.02 * h);
    // Forearms hang slightly forward of the upper arm.
    let fore_dir = (down + v(0.0, 0.0, 0.25)).normalize();
    let wrist = elbow + fore_dir * fore_l;
    let ua_r = 0.026 * h * limb;
    parts.push(
        SdfPart::add(
            "upper arm",
            capsule(sj, elbow, ua_r * 1.1, ua_r * 0.82),
            b(0.02),
        )
        .mirrored(),
    );
    parts.push(
        SdfPart::add(
            "biceps",
            ellipsoid(
                sj.lerp(elbow, 0.5) + v(0.0, 0.0, ua_r * 0.35),
                v(ua_r * 0.85, upper_l * 0.32, ua_r * 0.85),
            ),
            b(0.015),
        )
        .mirrored()
        .rotated(v(0.0, 0.0, spread)),
    );
    let fa_r = 0.023 * h * limb;
    parts.push(
        SdfPart::add(
            "forearm",
            capsule(elbow, wrist, fa_r * 1.05, fa_r * 0.66),
            b(0.015),
        )
        .mirrored(),
    );
    parts.push(
        SdfPart::add(
            "forearm muscle",
            ellipsoid(
                elbow.lerp(wrist, 0.28),
                v(fa_r * 1.05, fore_l * 0.3, fa_r * 0.95),
            ),
            b(0.012),
        )
        .mirrored()
        .rotated(v(0.0, 0.0, spread)),
    );
    // Hand frame: `d` along the hand, `n` out of the palm (toward the body),
    // `a` across the knuckles toward the thumb.
    let d = fore_dir;
    let n = (v(-1.0, 0.0, 0.0) - d * d.dot(v(-1.0, 0.0, 0.0))).normalize();
    let a = n.cross(d).normalize();
    let palm_len = hand_l * 0.5;
    let palm_w = hand_l * 0.5 * sp.hands.sqrt();
    let palm_t = hand_l * 0.2;
    let palm_c = wrist + d * (palm_len * 0.55);
    parts.push(
        SdfPart::add(
            "palm",
            rounded_box(palm_c, v(palm_t, palm_len, palm_w), palm_t * 0.45),
            b(0.01),
        )
        .mirrored()
        .rotated(euler_from_axes(-n, d, a)),
    );
    let finger_r = palm_w * 0.11;
    let finger_len = hand_l * 0.48;
    let knuckle_line = wrist + d * (palm_len * 1.0) - n * palm_t * 0.05;
    for (i, (label, len)) in [
        ("index finger", 0.95),
        ("middle finger", 1.0),
        ("ring finger", 0.94),
        ("little finger", 0.76),
    ]
    .into_iter()
    .enumerate()
    {
        let off = (1.5 - i as f32) * palm_w * 0.25;
        let base = knuckle_line + a * off;
        let l = finger_len * len;
        let splay = (1.5 - i as f32) * 4.0;
        let dir1 = (rot_about(d, n.cross(d), -splay) + n * 0.25).normalize();
        let mid = base + dir1 * l * 0.55;
        let dir2 = (dir1 + n * 0.55).normalize();
        let tip = mid + dir2 * l * 0.45;
        parts.push(
            SdfPart::add(
                label,
                capsule(base, mid, finger_r, finger_r * 0.9),
                b(0.004),
            )
            .mirrored(),
        );
        parts.push(
            SdfPart::add(
                &format!("{label} tip"),
                capsule(mid, tip, finger_r * 0.9, finger_r * 0.75),
                b(0.003),
            )
            .mirrored(),
        );
    }
    let thumb_base = wrist + d * (palm_len * 0.2) + a * (palm_w * 0.42) + n * (palm_t * 0.15);
    let thumb_dir = (d * 0.75 + a * 0.45 + n * 0.45).normalize();
    let thumb_mid = thumb_base + thumb_dir * finger_len * 0.5;
    let thumb_tip = thumb_mid + (thumb_dir + n * 0.4).normalize() * finger_len * 0.4;
    parts.push(
        SdfPart::add(
            "thumb",
            capsule(thumb_base, thumb_mid, finger_r * 1.35, finger_r * 1.0),
            b(0.008),
        )
        .mirrored(),
    );
    parts.push(
        SdfPart::add(
            "thumb tip",
            capsule(thumb_mid, thumb_tip, finger_r * 1.0, finger_r * 0.85),
            b(0.003),
        )
        .mirrored(),
    );

    // ---- Legs and feet -----------------------------------------------------
    let stance = 1.0 + bulk * 0.15;
    let hip_j = v(hip_w * 0.55, crotch + 0.03 * h, -0.005 * h);
    let knee = v(hip_w * 0.55 * stance, knee_y, 0.012 * h);
    let ankle = v(hip_w * 0.55 * stance * 1.02, ankle_y + 0.02 * h, -0.012 * h);
    let th_r = 0.05 * h * limb * (1.0 + fem * 0.12);
    parts.push(SdfPart::add("thigh", capsule(hip_j, knee, th_r, th_r * 0.58), b(0.03)).mirrored());
    parts.push(
        SdfPart::add(
            "thigh front muscle",
            ellipsoid(
                hip_j.lerp(knee, 0.55) + v(0.0, 0.0, th_r * 0.3),
                v(th_r * 0.75, (hip_j.y - knee.y) * 0.4, th_r * 0.7),
            ),
            b(0.02),
        )
        .mirrored(),
    );
    parts.push(
        SdfPart::add(
            "kneecap",
            ellipsoid(
                knee + v(0.0, 0.01 * h, th_r * 0.32),
                v(th_r * 0.3, th_r * 0.34, th_r * 0.22),
            ),
            b(0.02),
        )
        .mirrored(),
    );
    let calf_r = 0.03 * h * limb;
    parts.push(
        SdfPart::add(
            "shin",
            capsule(knee, ankle, th_r * 0.55, calf_r * 0.62),
            b(0.015),
        )
        .mirrored(),
    );
    parts.push(
        SdfPart::add(
            "calf",
            ellipsoid(
                knee.lerp(ankle, 0.3) + v(0.0, 0.0, -calf_r * 0.45),
                v(calf_r * 0.95, (knee.y - ankle.y) * 0.28, calf_r),
            ),
            b(0.02),
        )
        .mirrored(),
    );
    let foot_len = 0.15 * h * (sp.limb / 1.2).max(0.85).sqrt() * (1.0 - fem * 0.08);
    let foot_h = 0.045 * h;
    let foot_w = foot_len * 0.38;
    parts.push(
        SdfPart::add(
            "foot",
            rounded_box(
                v(ankle.x, foot_h * 0.5, ankle.z + foot_len * 0.28),
                v(foot_w, foot_h, foot_len * 0.9),
                foot_h * 0.45,
            ),
            b(0.02),
        )
        .mirrored(),
    );
    parts.push(
        SdfPart::add(
            "ankle",
            ellipsoid(
                ankle + v(0.0, -0.01 * h, 0.0),
                v(calf_r * 0.65, calf_r * 0.8, calf_r * 0.75),
            ),
            b(0.015),
        )
        .mirrored(),
    );
    parts.push(
        SdfPart::add(
            "toes",
            ellipsoid(
                v(ankle.x, foot_h * 0.35, ankle.z + foot_len * 0.68),
                v(foot_w * 0.55, foot_h * 0.4, foot_len * 0.16),
            ),
            b(0.012),
        )
        .mirrored(),
    );

    // ---- Head --------------------------------------------------------------
    let u = head_hh * sp.head_w; // head width unit
    let hc = v(0.0, chin + 0.5 * head_hh - headdrop, headz);
    let eye_y = chin + 0.5 * head_hh - headdrop + 0.02 * head_hh;
    parts.push(SdfPart::add(
        "cranium",
        ellipsoid(
            hc + v(0.0, 0.1 * head_hh, -0.06 * head_hh),
            v(0.36 * u, 0.42 * head_hh, 0.47 * head_hh),
        ),
        b(0.0) + 0.06 * head_hh,
    ));
    parts.push(SdfPart::add(
        "face",
        ellipsoid(
            hc + v(0.0, -0.08 * head_hh, 0.1 * head_hh),
            v(0.3 * u, 0.36 * head_hh, 0.32 * head_hh),
        ),
        0.08 * head_hh,
    ));
    let jaw_w = 0.27 * u * (0.9 + 0.45 * jaw);
    let jaw_fwd = jaw * 0.08 * head_hh;
    parts.push(SdfPart::add(
        "jaw",
        rounded_box(
            v(
                0.0,
                chin + 0.16 * head_hh - headdrop,
                headz + 0.1 * head_hh + jaw_fwd,
            ),
            v(jaw_w * 2.0, 0.26 * head_hh, 0.5 * head_hh),
            0.11 * head_hh,
        ),
        0.07 * head_hh,
    ));
    parts.push(SdfPart::add(
        "chin",
        ellipsoid(
            v(
                0.0,
                chin + 0.07 * head_hh - headdrop,
                headz + 0.32 * head_hh + jaw_fwd,
            ),
            v(0.11 * u * (1.0 + jaw * 0.5), 0.08 * head_hh, 0.08 * head_hh),
        ),
        0.06 * head_hh,
    ));
    parts.push(
        SdfPart::add(
            "cheekbone",
            ellipsoid(
                v(0.2 * u, eye_y - 0.1 * head_hh, headz + 0.24 * head_hh),
                v(0.1 * u, 0.065 * head_hh, 0.09 * head_hh),
            ),
            0.05 * head_hh,
        )
        .mirrored(),
    );
    let brow_r = 0.04 * head_hh * sp.brow;
    let brow_z = headz + 0.37 * head_hh + (sp.brow - 1.0) * 0.03 * head_hh;
    parts.push(
        SdfPart::add(
            "brow ridge",
            capsule(
                v(0.03 * u, eye_y + 0.1 * head_hh, brow_z),
                v(0.27 * u, eye_y + 0.11 * head_hh, brow_z - 0.1 * head_hh),
                brow_r,
                brow_r * 0.75,
            ),
            0.05 * head_hh,
        )
        .mirrored(),
    );
    let eye_x = 0.15 * u;
    let eye_r = 0.062 * head_hh;
    let eye_z = headz + 0.33 * head_hh;
    parts.push(
        SdfPart::add(
            "eye socket",
            ellipsoid(
                v(eye_x, eye_y, eye_z + 0.015 * head_hh),
                v(0.085 * u, 0.06 * head_hh, 0.07 * head_hh),
            ),
            0.04 * head_hh,
        )
        .subtract()
        .mirrored(),
    );
    // Eyelids wrap the top and bottom of the eyeballs (separate objects).
    let lid_c = v(eye_x, eye_y, eye_z - 0.015 * head_hh);
    parts.push(
        SdfPart::add(
            "upper eyelid",
            ellipsoid(
                lid_c + v(0.0, eye_r * 0.62, eye_r * 0.1),
                v(eye_r * 1.15, eye_r * 0.55, eye_r * 1.02),
            ),
            0.012 * head_hh,
        )
        .mirrored()
        .rotated(v(-18.0, 0.0, 0.0)),
    );
    parts.push(
        SdfPart::add(
            "lower eyelid",
            ellipsoid(
                lid_c + v(0.0, -eye_r * 0.72, eye_r * 0.02),
                v(eye_r * 1.08, eye_r * 0.4, eye_r * 0.98),
            ),
            0.012 * head_hh,
        )
        .mirrored(),
    );
    // Nose
    let nose_top = v(0.0, eye_y - 0.02 * head_hh, headz + 0.38 * head_hh);
    let (nose_tip, tip_r, wing) = match sp.nose {
        Nose::Human => (
            v(0.0, eye_y - 0.2 * head_hh, headz + 0.47 * head_hh),
            0.045,
            0.045,
        ),
        Nose::Flat => (
            v(0.0, eye_y - 0.17 * head_hh, headz + 0.47 * head_hh),
            0.06,
            0.07,
        ),
        Nose::Long => (
            v(0.0, eye_y - 0.24 * head_hh, headz + 0.66 * head_hh),
            0.045,
            0.045,
        ),
        Nose::Big => (
            v(0.0, eye_y - 0.22 * head_hh, headz + 0.5 * head_hh),
            0.08,
            0.06,
        ),
    };
    parts.push(SdfPart::add(
        "nose",
        capsule(nose_top, nose_tip, 0.035 * head_hh, tip_r * head_hh),
        0.035 * head_hh,
    ));
    parts.push(
        SdfPart::add(
            "nostril",
            ellipsoid(
                v(
                    wing * head_hh,
                    nose_tip.y + 0.005 * head_hh,
                    nose_tip.z - 0.06 * head_hh,
                ),
                v(wing * 0.8 * head_hh, 0.035 * head_hh, 0.045 * head_hh),
            ),
            0.025 * head_hh,
        )
        .mirrored(),
    );
    // Mouth: lips and the slit between them.
    let mouth_y = chin + 0.25 * head_hh - headdrop;
    let mouth_z = headz + 0.38 * head_hh + jaw_fwd * 0.5;
    parts.push(SdfPart::add(
        "upper lip",
        capsule(
            v(-0.1 * u, mouth_y + 0.01 * head_hh, mouth_z - 0.02 * head_hh),
            v(0.1 * u, mouth_y + 0.01 * head_hh, mouth_z - 0.02 * head_hh),
            0.03 * head_hh,
            0.03 * head_hh,
        ),
        0.03 * head_hh,
    ));
    parts.push(SdfPart::add(
        "lower lip",
        capsule(
            v(
                -0.09 * u * (1.0 + jaw * 0.4),
                mouth_y - 0.03 * head_hh,
                mouth_z - 0.02 * head_hh + jaw_fwd * 0.6,
            ),
            v(
                0.09 * u * (1.0 + jaw * 0.4),
                mouth_y - 0.03 * head_hh,
                mouth_z - 0.02 * head_hh + jaw_fwd * 0.6,
            ),
            0.032 * head_hh,
            0.032 * head_hh,
        ),
        0.03 * head_hh,
    ));
    parts.push(
        SdfPart::add(
            "mouth line",
            capsule(
                v(
                    -0.11 * u,
                    mouth_y - 0.008 * head_hh,
                    mouth_z + 0.03 * head_hh,
                ),
                v(
                    0.11 * u,
                    mouth_y - 0.008 * head_hh,
                    mouth_z + 0.03 * head_hh,
                ),
                0.009 * head_hh,
                0.009 * head_hh,
            ),
            0.012 * head_hh,
        )
        .subtract(),
    );
    // Ears
    let ear_root = v(0.36 * u, eye_y - 0.04 * head_hh, headz - 0.04 * head_hh);
    match ears {
        Ears::Round => parts.push(
            SdfPart::add(
                "ear",
                ellipsoid(
                    ear_root + v(0.02 * u, 0.0, -0.02 * head_hh),
                    v(0.045 * u, 0.13 * head_hh, 0.08 * head_hh),
                ),
                0.03 * head_hh,
            )
            .mirrored()
            .rotated(v(-10.0, -25.0, 0.0)),
        ),
        Ears::Pointed | Ears::Long => {
            let len = if ears == Ears::Long { 0.55 } else { 0.3 } * head_hh;
            // Long axis along local z, then swung out sideways and back, tilted up.
            let out = v(0.75, 0.35, -0.55).normalize();
            let center = ear_root + out * (len * 0.75);
            let x_axis = v(0.0, 1.0, 0.0).cross(out).normalize(); // flat side
            let y_axis = out.cross(x_axis).normalize();
            parts.push(
                SdfPart::add(
                    "ear",
                    ellipsoid(center, v(0.05 * head_hh, 0.11 * head_hh, len)),
                    0.04 * head_hh,
                )
                .mirrored()
                .rotated(euler_from_axes(x_axis, y_axis, out)),
            );
        }
    }

    // Flat soles: shave off anything below the ground.
    parts.push(
        SdfPart::add(
            "ground cut",
            rounded_box(v(0.0, -h, 0.0), v(h * 4.0, h * 2.0, h * 4.0), 0.0),
            0.004 * h,
        )
        .subtract(),
    );

    // ---- Landmarks ---------------------------------------------------------
    let eye_c = v(eye_x, eye_y, eye_z - 0.015 * head_hh);
    lm.insert("left_eye".into(), eye_c);
    lm.insert("right_eye".into(), v(-eye_c.x, eye_c.y, eye_c.z));
    lm.insert("head_center".into(), hc);
    lm.insert("nose_tip".into(), nose_tip + v(0.0, 0.0, tip_r * head_hh));
    lm.insert("mouth".into(), v(0.0, mouth_y, mouth_z + 0.03 * head_hh));
    lm.insert(
        "chin".into(),
        v(0.0, chin - headdrop, headz + 0.33 * head_hh + jaw_fwd),
    );
    let canine = v(
        0.075 * u * (1.0 + jaw * 0.3),
        mouth_y - 0.03 * head_hh,
        mouth_z + jaw_fwd * 0.6,
    );
    lm.insert("left_lower_canine".into(), canine);
    lm.insert(
        "right_lower_canine".into(),
        v(-canine.x, canine.y, canine.z),
    );
    lm.insert(
        "left_ear_tip".into(),
        ear_root + v(0.1 * u, 0.1 * head_hh, -0.05 * head_hh),
    );
    lm.insert("neck".into(), neck_bot.lerp(neck_top, 0.5));
    lm.insert("chest".into(), v(0.0, pec_y, 0.55 * sx + lean));
    lm.insert("waist".into(), v(0.0, crotch + 0.22 * torso, 0.0));
    lm.insert("pelvis".into(), pelvis_c);
    lm.insert("left_shoulder".into(), sj);
    lm.insert("right_shoulder".into(), v(-sj.x, sj.y, sj.z));
    lm.insert("left_elbow".into(), elbow);
    lm.insert("right_elbow".into(), v(-elbow.x, elbow.y, elbow.z));
    lm.insert("left_wrist".into(), wrist);
    lm.insert("right_wrist".into(), v(-wrist.x, wrist.y, wrist.z));
    let grip = palm_c + d * palm_len * 0.4 + n * palm_t * 1.4;
    lm.insert("left_hand_grip".into(), grip);
    lm.insert("right_hand_grip".into(), v(-grip.x, grip.y, grip.z));
    lm.insert("left_knee".into(), knee);
    lm.insert("right_knee".into(), v(-knee.x, knee.y, knee.z));
    lm.insert("left_ankle".into(), ankle);
    lm.insert("right_ankle".into(), v(-ankle.x, ankle.y, ankle.z));
    lm.insert(
        "left_toe".into(),
        v(ankle.x, 0.0, ankle.z + foot_len * 0.75),
    );

    // Points on the skin, found by marching out from inside the body, so
    // belts, bracers and armour can be sized to fit.
    let field = crate::sdf::Field::new(&parts)?;
    let surface = |from: Vec3, dir: Vec3| -> Vec3 {
        let dir = dir.normalize();
        let mut t = 0.0;
        for _ in 0..200 {
            let p = from + dir * t;
            if field.eval(p) > 0.0 {
                return p;
            }
            t += h * 0.002;
        }
        from
    };
    let waist = v(0.0, crotch + 0.22 * torso, 0.0);
    lm.insert("waist_front".into(), surface(waist, Vec3::Z));
    lm.insert("waist_back".into(), surface(waist, -Vec3::Z));
    lm.insert("waist_left".into(), surface(waist, Vec3::X));
    lm.insert("chest_front".into(), surface(v(0.0, pec_y, 0.0), Vec3::Z));
    let fore_mid = elbow.lerp(wrist, 0.55);
    lm.insert("left_forearm_mid".into(), fore_mid);
    lm.insert("left_forearm_outer".into(), surface(fore_mid, Vec3::X));
    let upper_mid = sj.lerp(elbow, 0.5);
    lm.insert("left_upper_arm_mid".into(), upper_mid);
    lm.insert("left_shoulder_top".into(), surface(sj, Vec3::Y));
    lm.insert("head_top".into(), surface(hc, Vec3::Y));

    Ok(Character {
        parts,
        landmarks: lm,
        eye_radius: eye_r,
        species,
        height: h,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sdf::{build, MeshOptions};

    #[test]
    fn every_species_builds_one_watertight_body() {
        for s in Species::ALL {
            let c = build_character(&CharacterParams {
                species: Some(s),
                ..Default::default()
            })
            .unwrap();
            let m = build(
                &c.parts,
                MeshOptions {
                    resolution: 64,
                    relax: 1,
                    fit_thin_parts: false,
                },
            )
            .unwrap();
            let b = m.bounds().unwrap();
            assert!(
                (b.max.y - c.height).abs() < c.height * 0.08,
                "{}: top at {} vs {}",
                s.name(),
                b.max.y,
                c.height
            );
            assert!(b.min.y > -0.01 * c.height, "{}: below the ground", s.name());
            assert!(
                (b.max.x + b.min.x).abs() < 0.02 * c.height,
                "{}: not symmetric {:?}",
                s.name(),
                b
            );
            assert!(m.volume() > 0.0);
        }
    }

    #[test]
    fn options_change_the_body() {
        let base = build_character(&CharacterParams {
            species: Some(Species::Human),
            ..Default::default()
        })
        .unwrap();
        let apose = build_character(&CharacterParams {
            species: Some(Species::Human),
            pose: Some(Pose::APose),
            ..Default::default()
        })
        .unwrap();
        assert!(apose.landmarks["left_wrist"].x > base.landmarks["left_wrist"].x + 0.1);
        assert!(
            base.landmarks["left_eye"].x > 0.0,
            "the character's left is +x"
        );
    }
}

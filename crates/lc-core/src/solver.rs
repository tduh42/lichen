//! Search solver (protocol.md §11, ADR-0016): a particle filter over a target's position, from
//! RSSI ranges, bearings and the target's own fixes, on a local east-north plane.
//!
//! Deterministic: every random draw comes from a seed, so the same observations give the same
//! estimate, and the whole filter is run again over every observation when one is added (a few
//! hundred observations of 2,000 particles: milliseconds). Floats are fine here (`std` crate).

use rand_chacha::ChaCha8Rng;
use rand_core::{Rng, SeedableRng};

/// Metres per degree of latitude.
const M_PER_DEG: f64 = 111_320.0;
/// Particles of a solve.
pub const PARTICLES: usize = 2_000;
/// Heat grid side, in cells.
pub const HEAT_CELLS: u16 = 40;
/// Furthest range considered, metres (a LoRa link on the ground rarely goes beyond).
const MAX_RANGE_M: f64 = 30_000.0;
/// Likelihood floors (outliers): a reading this far off still leaves the particle a chance.
const FLOOR_RSSI: f64 = 1e-3;
const FLOOR_BEARING: f64 = 0.05;
const FLOOR_FIX: f64 = 1e-6;
/// Share of the particles drawn again from the current observation at each resampling
/// (recovery from a wrong start).
const INJECT_PER_MILLE: u64 = 10;
/// Correlation time of the target's walk, seconds.
const WALK_S: f64 = 60.0;
/// Below this effective number of particles after a fix, the filter starts again from the fix.
const MIN_FIX_ESS: f64 = 100.0;
/// 68 % and 95 % of a 2-D Gaussian lie within these many standard deviations (along each
/// semi-axis): `sqrt(−2 ln(1 − p))`.
const K68: f64 = 1.509_6;
const K95: f64 = 2.447_7;

/// Path loss: `RSSI = tx + a − 10·n·log10(d)`, `d` in metres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PathLoss {
    /// RSSI at 1 m minus the TX power, dB: about −32 at 869 MHz (free space over the first
    /// metre, antennas of 0 dBi).
    pub a_db: f64,
    /// Path loss exponent: 2 in free space, 2.7 to 3.5 near the ground.
    pub n: f64,
    /// Shadowing and fading, dB (one standard deviation).
    pub sigma_db: f64,
}

impl PathLoss {
    /// Literature defaults until the field test (L8c): n = 2.8, σ = 8 dB.
    pub const DEFAULT: Self = Self {
        a_db: -32.0,
        n: 2.8,
        sigma_db: 8.0,
    };

    /// The RSSI expected at `d_m` metres.
    pub fn rssi(&self, tx_dbm: f64, d_m: f64) -> f64 {
        tx_dbm + self.a_db - 10.0 * self.n * d_m.max(1.0).log10()
    }

    /// The distance at which `rssi` is expected.
    pub fn distance(&self, tx_dbm: f64, rssi: f64) -> f64 {
        10f64
            .powf((tx_dbm + self.a_db - rssi) / (10.0 * self.n))
            .clamp(1.0, MAX_RANGE_M)
    }
}

/// What was observed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    /// A direct reception of the target by the observer.
    Rssi {
        rssi: f64,
        /// The target's TX power then.
        tx_dbm: f64,
        /// Uncertainty beyond the model's shadowing (an estimated RSSI, an unknown TX power).
        extra_sigma_db: f64,
        /// Saturated: the true RSSI is at least `rssi`.
        lower_bound: bool,
    },
    /// A bearing from the observer to the target, degrees from true north.
    Bearing { deg: f64, sigma_deg: f64 },
    /// The target's own position (`lat`, `lon` are the target's), ± radius (68 %).
    Fix { radius_m: f64 },
}

/// An observation: where the observer was (the target, for a fix), when, and what it saw.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Observation {
    /// Unix seconds.
    pub t: f64,
    pub lat: f64,
    pub lon: f64,
    pub kind: Kind,
}

/// Solver settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Params {
    pub model: PathLoss,
    /// How fast the target may move (random walk), m/s: slow by default (someone in distress
    /// moves little; a target that walks reports fixes, which dominate).
    pub speed_mps: f64,
    pub particles: usize,
    pub seed: u64,
}

impl Params {
    pub const fn new(model: PathLoss, seed: u64) -> Self {
        Self {
            model,
            speed_mps: 0.5,
            particles: PARTICLES,
            seed,
        }
    }
}

/// The probability over a square around the estimate, for the map.
#[derive(Clone, Debug, PartialEq)]
pub struct Heat {
    pub north: f64,
    pub south: f64,
    pub west: f64,
    pub east: f64,
    pub cols: u16,
    pub rows: u16,
    /// Row-major from the north-west corner; 255 is the densest cell.
    pub cells: Vec<u8>,
}

/// Where the target probably is.
#[derive(Clone, Debug, PartialEq)]
pub struct Estimate {
    pub lat: f64,
    pub lon: f64,
    /// Ellipses holding 68 % and 95 % of the probability (Gaussian approximation): semi-axes in
    /// metres, the major axis's azimuth in degrees from north (0..180).
    pub major68_m: f64,
    pub minor68_m: f64,
    pub major95_m: f64,
    pub minor95_m: f64,
    pub azimuth_deg: f64,
    pub heat: Heat,
    pub observations: usize,
    /// The path loss used (defaults, or fitted to the target's fixes).
    pub model: PathLoss,
}

/// Runs the filter over `obs` (sorted by time). `None` without observations.
pub fn solve(obs: &[Observation], p: &Params) -> Option<Estimate> {
    let first = obs.first()?;
    let plane = Plane::new(first.lat, first.lon);
    let mut rng = ChaCha8Rng::seed_from_u64(p.seed);
    let n = p.particles.max(10);
    let mut px = vec![0.0; n];
    let mut py = vec![0.0; n];
    let o0 = Local::of(first, &plane);
    for i in 0..n {
        let (x, y) = propose(&o0, &p.model, &mut rng);
        px[i] = x;
        py[i] = y;
    }
    let mut lw = vec![0.0f64; n];
    let mut t_prev = first.t;
    for o in obs.iter().skip(1) {
        let o = Local::of(o, &plane);
        // Resample when the weights have collapsed, then roughen and draw a few particles from
        // this observation (recovery from a wrong start).
        let w = normalized(&lw);
        let ess = 1.0 / w.iter().map(|w| w * w).sum::<f64>();
        if ess < n as f64 / 2.0 {
            let (sx, sy) = spread(&px, &py, &w);
            resample(&mut px, &mut py, &w, &mut rng);
            let rough = (0.05 * sx.max(sy)).max(2.0);
            for i in 0..n {
                px[i] += rough * gauss(&mut rng);
                py[i] += rough * gauss(&mut rng);
            }
            if matches!(
                o.kind,
                Kind::Fix { .. }
                    | Kind::Rssi {
                        lower_bound: false,
                        ..
                    }
            ) {
                for i in 0..n {
                    if rng.next_u64() % 1000 < INJECT_PER_MILLE {
                        (px[i], py[i]) = propose(&o, &p.model, &mut rng);
                    }
                }
            }
            lw.iter_mut().for_each(|x| *x = 0.0);
        }
        // The target moved: a random walk whose spread grows with the square root of time
        // (about `speed × 60 s` per minute, wherever the observations fall).
        let dt = (o.t - t_prev).max(0.0);
        t_prev = t_prev.max(o.t);
        let step = (p.speed_mps * (WALK_S * dt).sqrt()).min(300.0) / std::f64::consts::SQRT_2;
        if step > 0.0 {
            for i in 0..n {
                px[i] += step * gauss(&mut rng);
                py[i] += step * gauss(&mut rng);
            }
        }
        for i in 0..n {
            lw[i] += ln_likelihood(&o, &p.model, px[i], py[i]);
        }
        // A fix much sharper than the cloud leaves too few particles near it: start again from
        // the fix (it dominates whatever came before).
        if matches!(o.kind, Kind::Fix { .. }) {
            let w = normalized(&lw);
            let ess = 1.0 / w.iter().map(|w| w * w).sum::<f64>();
            if ess < MIN_FIX_ESS {
                for i in 0..n {
                    (px[i], py[i]) = propose(&o, &p.model, &mut rng);
                }
                lw.iter_mut().for_each(|x| *x = 0.0);
            }
        }
    }
    let w = normalized(&lw);
    Some(summarize(&px, &py, &w, &plane, obs.len(), p.model))
}

/// Fits the path loss to receptions at known distances: `(distance m, RSSI − TX dBm)`, from
/// the target's own fixes. Two pairs give the offset (default exponent), four spread over
/// distance the exponent too, six the spread.
pub fn calibrate(pairs: &[(f64, f64)], default: PathLoss) -> Option<PathLoss> {
    let pts: Vec<(f64, f64)> = pairs
        .iter()
        .filter(|(d, y)| *d >= 5.0 && d.is_finite() && y.is_finite())
        .map(|(d, y)| (-10.0 * d.log10(), *y))
        .collect();
    if pts.len() < 2 {
        return None;
    }
    let m = pts.len() as f64;
    let mx = pts.iter().map(|p| p.0).sum::<f64>() / m;
    let my = pts.iter().map(|p| p.1).sum::<f64>() / m;
    let sxx = pts.iter().map(|p| (p.0 - mx).powi(2)).sum::<f64>() / m;
    let sxy = pts.iter().map(|p| (p.0 - mx) * (p.1 - my)).sum::<f64>() / m;
    // log10 spread of 0.1: distances varying by a factor of about 1.3 at least.
    let n = if pts.len() >= 4 && sxx >= 1.0 {
        (sxy / sxx).clamp(1.6, 5.0)
    } else {
        default.n
    };
    let a_db = (my - n * mx).clamp(-70.0, -5.0);
    let sigma_db = if pts.len() >= 6 {
        let r2 = pts
            .iter()
            .map(|p| (p.1 - (a_db + n * p.0)).powi(2))
            .sum::<f64>()
            / (m - 2.0);
        r2.sqrt().clamp(3.0, 12.0)
    } else {
        default.sigma_db
    };
    Some(PathLoss { a_db, n, sigma_db })
}

/// Bearing (degrees from north) and distance (metres) from `from` to `to`, in degrees.
pub fn bearing_distance(from: (f64, f64), to: (f64, f64)) -> (f64, f64) {
    let plane = Plane::new(from.0, from.1);
    let (x, y) = plane.xy(to.0, to.1);
    let deg = x.atan2(y).to_degrees().rem_euclid(360.0);
    (deg, x.hypot(y))
}

// ----- internals -------------------------------------------------------------------------------

/// A local east-north plane around an origin (equirectangular: exact enough over tens of km).
struct Plane {
    lat0: f64,
    lon0: f64,
    cos0: f64,
}

impl Plane {
    fn new(lat0: f64, lon0: f64) -> Self {
        Self {
            lat0,
            lon0,
            cos0: lat0.to_radians().cos().max(1e-6),
        }
    }

    fn xy(&self, lat: f64, lon: f64) -> (f64, f64) {
        let mut dlon = lon - self.lon0;
        if dlon > 180.0 {
            dlon -= 360.0;
        } else if dlon < -180.0 {
            dlon += 360.0;
        }
        (dlon * M_PER_DEG * self.cos0, (lat - self.lat0) * M_PER_DEG)
    }

    fn latlon(&self, x: f64, y: f64) -> (f64, f64) {
        (
            self.lat0 + y / M_PER_DEG,
            self.lon0 + x / (M_PER_DEG * self.cos0),
        )
    }
}

/// An observation on the plane.
struct Local {
    t: f64,
    x: f64,
    y: f64,
    kind: Kind,
}

impl Local {
    fn of(o: &Observation, plane: &Plane) -> Self {
        let (x, y) = plane.xy(o.lat, o.lon);
        Self {
            t: o.t,
            x,
            y,
            kind: o.kind,
        }
    }
}

fn uniform(rng: &mut ChaCha8Rng) -> f64 {
    (rng.next_u64() >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
}

/// A standard normal draw (Box-Muller).
fn gauss(rng: &mut ChaCha8Rng) -> f64 {
    let u = uniform(rng).max(1e-300);
    let v = uniform(rng);
    (-2.0 * u.ln()).sqrt() * (std::f64::consts::TAU * v).cos()
}

/// A position drawn from what one observation says alone.
fn propose(o: &Local, model: &PathLoss, rng: &mut ChaCha8Rng) -> (f64, f64) {
    let theta = std::f64::consts::TAU * uniform(rng);
    match o.kind {
        Kind::Fix { radius_m } => {
            let s = radius_m.max(1.0);
            (o.x + s * gauss(rng), o.y + s * gauss(rng))
        }
        Kind::Rssi {
            rssi,
            tx_dbm,
            extra_sigma_db,
            lower_bound,
        } => {
            let d0 = model.distance(tx_dbm, rssi);
            let d = if lower_bound {
                // At most about that far: anywhere in the disc, a little beyond.
                1.5 * d0 * uniform(rng).sqrt()
            } else {
                let s = model.sigma_db.hypot(extra_sigma_db);
                (d0 * 10f64.powf(s * gauss(rng) / (10.0 * model.n))).min(MAX_RANGE_M)
            };
            (o.x + d * theta.sin(), o.y + d * theta.cos())
        }
        Kind::Bearing { deg, sigma_deg } => {
            let a = (deg + sigma_deg * gauss(rng)).to_radians();
            let d = 5_000.0 * uniform(rng);
            (o.x + d * a.sin(), o.y + d * a.cos())
        }
    }
}

/// `ln P(observation | target at (x, y))`, up to a constant.
fn ln_likelihood(o: &Local, model: &PathLoss, x: f64, y: f64) -> f64 {
    let (dx, dy) = (x - o.x, y - o.y);
    let d = dx.hypot(dy);
    match o.kind {
        Kind::Rssi {
            rssi,
            tx_dbm,
            extra_sigma_db,
            lower_bound,
        } => {
            let s = model.sigma_db.hypot(extra_sigma_db);
            let z = (rssi - model.rssi(tx_dbm, d)) / s;
            let l = if lower_bound {
                // P(true ≥ rssi) = Φ((predicted − rssi) / σ).
                phi(-z)
            } else {
                (-0.5 * z * z).exp()
            };
            (l + FLOOR_RSSI).ln()
        }
        Kind::Bearing { deg, sigma_deg } => {
            if d < 5.0 {
                return 0.0;
            }
            let predicted = dx.atan2(dy).to_degrees();
            let delta = (deg - predicted + 180.0).rem_euclid(360.0) - 180.0;
            let z = delta / sigma_deg.max(1.0);
            ((-0.5 * z * z).exp() + FLOOR_BEARING).ln()
        }
        Kind::Fix { radius_m } => {
            let z = d / radius_m.max(1.0);
            ((-0.5 * z * z).exp() + FLOOR_FIX).ln()
        }
    }
}

/// The standard normal CDF (Abramowitz and Stegun 7.1.26, error below 1.5e-7).
fn phi(x: f64) -> f64 {
    let z = x.abs() / std::f64::consts::SQRT_2;
    let t = 1.0 / (1.0 + 0.327_591_1 * z);
    let poly = t
        * (0.254_829_592
            + t * (-0.284_496_736
                + t * (1.421_413_741 + t * (-1.453_152_027 + t * 1.061_405_429))));
    let erf = 1.0 - poly * (-z * z).exp();
    if x >= 0.0 {
        0.5 * (1.0 + erf)
    } else {
        0.5 * (1.0 - erf)
    }
}

/// A longitude in −180..180.
fn wrap_lon(lon: f64) -> f64 {
    (lon + 180.0).rem_euclid(360.0) - 180.0
}

/// Weights from log-weights, summing to 1.
fn normalized(lw: &[f64]) -> Vec<f64> {
    let max = lw.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let mut w: Vec<f64> = lw.iter().map(|l| (l - max).exp()).collect();
    let sum: f64 = w.iter().sum();
    if sum > 0.0 && sum.is_finite() {
        w.iter_mut().for_each(|x| *x /= sum);
    } else {
        let u = 1.0 / w.len() as f64;
        w.iter_mut().for_each(|x| *x = u);
    }
    w
}

/// Weighted standard deviations along x and y.
fn spread(px: &[f64], py: &[f64], w: &[f64]) -> (f64, f64) {
    let mx: f64 = px.iter().zip(w).map(|(x, w)| x * w).sum();
    let my: f64 = py.iter().zip(w).map(|(y, w)| y * w).sum();
    let vx: f64 = px.iter().zip(w).map(|(x, w)| w * (x - mx).powi(2)).sum();
    let vy: f64 = py.iter().zip(w).map(|(y, w)| w * (y - my).powi(2)).sum();
    (vx.sqrt(), vy.sqrt())
}

/// Systematic resampling.
fn resample(px: &mut Vec<f64>, py: &mut Vec<f64>, w: &[f64], rng: &mut ChaCha8Rng) {
    let n = w.len();
    let step = 1.0 / n as f64;
    let mut u = uniform(rng) * step;
    let (mut nx, mut ny) = (Vec::with_capacity(n), Vec::with_capacity(n));
    let mut c = 0.0;
    let mut i = 0;
    for (j, wj) in w.iter().enumerate() {
        c += wj;
        while u < c && i < n {
            nx.push(px[j]);
            ny.push(py[j]);
            u += step;
            i += 1;
        }
    }
    // Rounding can leave the last slots empty.
    while nx.len() < n {
        nx.push(px[n - 1]);
        ny.push(py[n - 1]);
    }
    *px = nx;
    *py = ny;
}

fn summarize(
    px: &[f64],
    py: &[f64],
    w: &[f64],
    plane: &Plane,
    observations: usize,
    model: PathLoss,
) -> Estimate {
    let mx: f64 = px.iter().zip(w).map(|(x, w)| x * w).sum();
    let my: f64 = py.iter().zip(w).map(|(y, w)| y * w).sum();
    let (mut cxx, mut cxy, mut cyy) = (0.0, 0.0, 0.0);
    for i in 0..w.len() {
        let (dx, dy) = (px[i] - mx, py[i] - my);
        cxx += w[i] * dx * dx;
        cxy += w[i] * dx * dy;
        cyy += w[i] * dy * dy;
    }
    let half = (cxx + cyy) / 2.0;
    let root = (((cxx - cyy) / 2.0).powi(2) + cxy * cxy).sqrt();
    let (l1, l2) = ((half + root).max(0.0), (half - root).max(0.0));
    // Angle of the major axis from the x (east) axis, then as an azimuth from north.
    let phi_x = 0.5 * (2.0 * cxy).atan2(cxx - cyy);
    let azimuth_deg = (90.0 - phi_x.to_degrees()).rem_euclid(180.0);
    let (lat, lon) = plane.latlon(mx, my);

    // Heat grid: a square of ±3 σ (major axis) around the mean, splatted then blurred.
    let side = HEAT_CELLS as usize;
    let half_m = (3.0 * l1.sqrt()).clamp(50.0, MAX_RANGE_M);
    let cell = 2.0 * half_m / side as f64;
    let mut acc = vec![0.0f64; side * side];
    for i in 0..w.len() {
        let c = ((px[i] - (mx - half_m)) / cell).floor();
        let r = (((my + half_m) - py[i]) / cell).floor();
        if c >= 0.0 && r >= 0.0 && (c as usize) < side && (r as usize) < side {
            acc[r as usize * side + c as usize] += w[i];
        }
    }
    let mut blurred = vec![0.0f64; side * side];
    for r in 0..side {
        for c in 0..side {
            let mut s = 0.0;
            for (dr, wr) in [(-1i64, 1.0), (0, 2.0), (1, 1.0)] {
                for (dc, wc) in [(-1i64, 1.0), (0, 2.0), (1, 1.0)] {
                    let (rr, cc) = (r as i64 + dr, c as i64 + dc);
                    if rr >= 0 && cc >= 0 && (rr as usize) < side && (cc as usize) < side {
                        s += wr * wc * acc[rr as usize * side + cc as usize];
                    }
                }
            }
            blurred[r * side + c] = s;
        }
    }
    let peak = blurred.iter().copied().fold(0.0, f64::max);
    let cells = blurred
        .iter()
        .map(|v| {
            if peak > 0.0 {
                (255.0 * v / peak).round() as u8
            } else {
                0
            }
        })
        .collect();
    let (north, west) = plane.latlon(mx - half_m, my + half_m);
    let (south, east) = plane.latlon(mx + half_m, my - half_m);
    // A place a map takes (review of 2026-11-15, SR-37): near a pole the plane runs past ±90°
    // and its columns span more than the globe, and the mean may cross the antimeridian.
    // Android's map rejects a latitude beyond ±90° outright.
    let (lat, lon) = (lat.clamp(-90.0, 90.0), wrap_lon(lon));
    let (north, south) = (north.clamp(-90.0, 90.0), south.clamp(-90.0, 90.0));
    let (west, east) = if east - west >= 360.0 {
        (-180.0, 180.0)
    } else {
        let w = wrap_lon(west);
        (w, w + (east - west))
    };
    Estimate {
        lat,
        lon,
        major68_m: K68 * l1.sqrt(),
        minor68_m: K68 * l2.sqrt(),
        major95_m: K95 * l1.sqrt(),
        minor95_m: K95 * l2.sqrt(),
        azimuth_deg,
        heat: Heat {
            north,
            south,
            west,
            east,
            cols: HEAT_CELLS,
            rows: HEAT_CELLS,
            cells,
        },
        observations,
        model,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORIGIN: (f64, f64) = (45.0, 5.5);

    /// A point `east`, `north` metres from the origin, in degrees.
    fn at(east: f64, north: f64) -> (f64, f64) {
        Plane::new(ORIGIN.0, ORIGIN.1).latlon(east, north)
    }

    fn error_m(e: &Estimate, truth: (f64, f64)) -> f64 {
        bearing_distance((e.lat, e.lon), truth).1
    }

    /// A searcher's start (east, north, metres) and velocity (m/s).
    type Walk = ((f64, f64), (f64, f64));

    /// Receptions of a target at `target` by searchers walking `paths`, every 60 s, with
    /// shadowing `sigma` dB.
    fn simulate(
        target: (f64, f64),
        paths: &[Walk],
        steps: usize,
        sigma: f64,
        seed: u64,
    ) -> Vec<Observation> {
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        let model = PathLoss::DEFAULT;
        let mut obs = Vec::new();
        for k in 0..steps {
            for (s, ((x0, y0), (vx, vy))) in paths.iter().enumerate() {
                let t = 1_800_000_000.0 + 60.0 * k as f64 + s as f64;
                let (x, y) = (x0 + vx * 60.0 * k as f64, y0 + vy * 60.0 * k as f64);
                let d = (x - target.0).hypot(y - target.1);
                let rssi = model.rssi(22.0, d) + sigma * gauss(&mut rng);
                let (lat, lon) = at(x, y);
                obs.push(Observation {
                    t,
                    lat,
                    lon,
                    kind: Kind::Rssi {
                        rssi: rssi.round(),
                        tx_dbm: 22.0,
                        extra_sigma_db: 0.0,
                        lower_bound: false,
                    },
                });
            }
        }
        obs
    }

    /// Review of 2026-11-15, SR-37 (found by `search_solve`): a group member can send positions
    /// anywhere, the poles and the antimeridian included; the estimate stays a place a map can
    /// show (latitude within ±90°, longitude within ±180°), heat grid included.
    #[test]
    fn estimates_stay_on_the_map() {
        let weak = Kind::Rssi {
            rssi: -125.0,
            tx_dbm: 22.0,
            extra_sigma_db: 0.0,
            lower_bound: false,
        };
        for (lat, lon) in [(89.99, 0.0), (-89.99, 10.0), (0.0, 179.99), (60.0, -179.99)] {
            let obs: Vec<Observation> = (0..20)
                .map(|k| Observation {
                    t: 1_800_000_000.0 + 60.0 * f64::from(k),
                    lat,
                    lon,
                    kind: weak,
                })
                .collect();
            let e = solve(&obs, &Params::new(PathLoss::DEFAULT, 3)).unwrap();
            assert!((-90.0..=90.0).contains(&e.lat), "{lat}, {lon}: {}", e.lat);
            assert!((-180.0..=180.0).contains(&e.lon), "{lat}, {lon}: {}", e.lon);
            let h = &e.heat;
            assert!(
                (-90.0..=90.0).contains(&h.north) && (-90.0..=90.0).contains(&h.south),
                "{lat}, {lon}: {h:?}"
            );
        }
    }

    #[test]
    fn model_round_trip() {
        let m = PathLoss::DEFAULT;
        for d in [10.0, 300.0, 4_000.0] {
            assert!((m.distance(22.0, m.rssi(22.0, d)) - d).abs() < 1e-6 * d);
        }
        assert!((phi(0.0) - 0.5).abs() < 1e-9);
        assert!((phi(1.96) - 0.975).abs() < 1e-3);
        assert!((phi(-1.96) - 0.025).abs() < 1e-3);
    }

    #[test]
    fn bearing_and_distance() {
        let (b, d) = bearing_distance(ORIGIN, at(1000.0, 0.0));
        assert!((b - 90.0).abs() < 0.1 && (d - 1000.0).abs() < 1.0);
        let (b, d) = bearing_distance(ORIGIN, at(-500.0, -500.0));
        assert!((b - 225.0).abs() < 0.1 && (d - 707.1).abs() < 1.0);
    }

    #[test]
    fn a_fix_dominates() {
        let truth = at(400.0, -250.0);
        let mut obs = simulate((400.0, -250.0), &[((-800.0, 0.0), (0.0, 0.0))], 3, 8.0, 1);
        obs.push(Observation {
            t: 1_800_000_500.0,
            lat: truth.0,
            lon: truth.1,
            kind: Kind::Fix { radius_m: 10.0 },
        });
        let e = solve(&obs, &Params::new(PathLoss::DEFAULT, 7)).unwrap();
        assert!(error_m(&e, truth) < 15.0, "{}", error_m(&e, truth));
        assert!(e.major68_m < 30.0, "{}", e.major68_m);
    }

    #[test]
    fn same_seed_same_estimate() {
        let obs = simulate((300.0, 300.0), &[((0.0, 0.0), (1.0, 0.0))], 5, 8.0, 3);
        let p = Params::new(PathLoss::DEFAULT, 11);
        assert_eq!(solve(&obs, &p), solve(&obs, &p));
        assert!(solve(&[], &p).is_none());
    }

    /// Three searchers walking around a target 1 km away, shadowing 6 dB: after 15 rounds the
    /// estimate is within a few hundred metres and its 95 % ellipse holds the truth.
    #[test]
    fn three_searchers_converge() {
        let target = (600.0, 900.0);
        let paths = [
            ((-200.0, -200.0), (1.2, 0.0)),
            ((1_400.0, 0.0), (0.0, 1.2)),
            ((0.0, 1_800.0), (1.0, -0.5)),
        ];
        let mut misses = 0;
        let mut errors = Vec::new();
        for seed in 0..8 {
            let obs = simulate(target, &paths, 15, 6.0, 100 + seed);
            let e = solve(&obs, &Params::new(PathLoss::DEFAULT, seed)).unwrap();
            let err = error_m(&e, at(target.0, target.1));
            errors.push(err.round());
            if err > e.major95_m {
                misses += 1;
            }
        }
        errors.sort_by(f64::total_cmp);
        let median = errors[errors.len() / 2];
        assert!(median < 300.0, "{errors:?}");
        assert!(misses <= 2, "{misses} misses, {errors:?}");
    }

    /// Bearings narrow the estimate down.
    #[test]
    fn bearings_help() {
        let target = (500.0, 1_200.0);
        let paths = [((0.0, 0.0), (0.5, 0.0)), ((1_500.0, 0.0), (0.0, 0.5))];
        let mut with = 0.0;
        let mut without = 0.0;
        for seed in 0..6 {
            let obs = simulate(target, &paths, 6, 8.0, 200 + seed);
            let p = Params::new(PathLoss::DEFAULT, seed);
            let e = solve(&obs, &p).unwrap();
            without += error_m(&e, at(target.0, target.1));
            let mut rng = ChaCha8Rng::seed_from_u64(seed);
            let mut more = obs.clone();
            for o in &obs {
                let (b, _) = bearing_distance((o.lat, o.lon), at(target.0, target.1));
                more.push(Observation {
                    t: o.t + 0.5,
                    kind: Kind::Bearing {
                        deg: (b + 15.0 * gauss(&mut rng)).rem_euclid(360.0),
                        sigma_deg: 15.0,
                    },
                    ..*o
                });
            }
            more.sort_by(|a, b| a.t.total_cmp(&b.t));
            let e = solve(&more, &p).unwrap();
            with += error_m(&e, at(target.0, target.1));
        }
        assert!(with < 0.6 * without, "with {with}, without {without}");
    }

    /// A saturated reading close by bounds the distance.
    #[test]
    fn saturated_bounds_the_distance() {
        let (lat, lon) = at(0.0, 0.0);
        let obs = [Observation {
            t: 0.0,
            lat,
            lon,
            kind: Kind::Rssi {
                rssi: -70.0,
                tx_dbm: 22.0,
                extra_sigma_db: 3.0,
                lower_bound: true,
            },
        }];
        let e = solve(&obs, &Params::new(PathLoss::DEFAULT, 1)).unwrap();
        let bound = PathLoss::DEFAULT.distance(22.0, -70.0);
        assert!(error_m(&e, (lat, lon)) < 0.3 * bound);
        assert!(e.major95_m < 2.0 * bound, "{} vs {bound}", e.major95_m);
    }

    #[test]
    fn calibration_recovers_the_model() {
        let truth = PathLoss {
            a_db: -40.0,
            n: 3.3,
            sigma_db: 5.0,
        };
        let mut rng = ChaCha8Rng::seed_from_u64(5);
        let pairs: Vec<(f64, f64)> = (0..30)
            .map(|i| {
                let d = 50.0 * 1.15f64.powi(i);
                (d, truth.rssi(0.0, d) + 5.0 * gauss(&mut rng))
            })
            .collect();
        let m = calibrate(&pairs, PathLoss::DEFAULT).unwrap();
        assert!((m.n - 3.3).abs() < 0.25, "{m:?}");
        assert!((m.a_db + 40.0).abs() < 6.0, "{m:?}");
        assert!((m.sigma_db - 5.0).abs() < 1.5, "{m:?}");
        // Two pairs: the offset only.
        let m = calibrate(&pairs[..2], PathLoss::DEFAULT).unwrap();
        assert_eq!(m.n, PathLoss::DEFAULT.n);
        assert!(calibrate(&pairs[..1], PathLoss::DEFAULT).is_none());
    }

    #[test]
    fn heat_peaks_near_the_estimate() {
        let obs = simulate(
            (300.0, -400.0),
            &[((0.0, 0.0), (1.0, 0.0)), ((900.0, 0.0), (0.0, -1.0))],
            10,
            6.0,
            9,
        );
        let e = solve(&obs, &Params::new(PathLoss::DEFAULT, 2)).unwrap();
        let h = &e.heat;
        assert_eq!(h.cells.len(), usize::from(h.cols) * usize::from(h.rows));
        assert!(h.north > e.lat && e.lat > h.south && h.west < e.lon && e.lon < h.east);
        let peak = h.cells.iter().position(|&c| c == 255).unwrap();
        let (r, c) = (peak / usize::from(h.cols), peak % usize::from(h.cols));
        let lat = h.north - (r as f64 + 0.5) * (h.north - h.south) / f64::from(h.rows);
        let lon = h.west + (c as f64 + 0.5) * (h.east - h.west) / f64::from(h.cols);
        let d = bearing_distance((e.lat, e.lon), (lat, lon)).1;
        assert!(d < e.major95_m, "{d} vs {}", e.major95_m);
    }
}

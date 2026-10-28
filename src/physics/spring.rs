#![allow(dead_code)]

//! Mass-Spring-Damper Kinetic Simulation
//! Provides physically-based spring animations with natural bounce and settling.

#[derive(Debug, Clone, Copy)]
pub struct Spring {
    pub current: f32,
    pub target: f32,
    pub velocity: f32,
    pub stiffness: f32,
    pub damping: f32,
    pub mass: f32,
    pub precision: f32,
}

impl Spring {
    pub fn new(initial: f32, stiffness: f32, damping: f32, mass: f32) -> Self {
        Self {
            current: initial,
            target: initial,
            velocity: 0.0,
            stiffness,
            damping,
            mass: mass.max(0.001),
            precision: 0.08,
        }
    }

    pub fn set_target(&mut self, target: f32) {
        self.target = target;
    }

    pub fn snap_to(&mut self, val: f32) {
        self.current = val;
        self.target = val;
        self.velocity = 0.0;
    }

    pub fn is_settled(&self) -> bool {
        (self.current - self.target).abs() < self.precision && self.velocity.abs() < self.precision
    }

    /// Advances the spring simulation by dt (seconds).
    /// Returns true if still animating, or false if settled.
    pub fn update(&mut self, dt: f32) -> bool {
        if self.is_settled() {
            self.current = self.target;
            self.velocity = 0.0;
            return false;
        }

        // Substep integration for numerical stability if dt is relatively large
        let substeps = 4;
        let sub_dt = dt / (substeps as f32);

        for _ in 0..substeps {
            // Spring force: F_spring = -k * (x - target)
            let f_spring = -self.stiffness * (self.current - self.target);
            // Damping force: F_damping = -c * velocity
            let f_damping = -self.damping * self.velocity;
            // Total force = F_spring + F_damping
            let accel = (f_spring + f_damping) / self.mass;

            // Semi-implicit Euler
            self.velocity += accel * sub_dt;
            self.current += self.velocity * sub_dt;
        }

        if self.is_settled() {
            self.current = self.target;
            self.velocity = 0.0;
            false
        } else {
            true
        }
    }
}

/// Compound spring animator for Island geometry
#[derive(Debug, Clone)]
pub struct SpringRect {
    pub x: Spring,
    pub y: Spring,
    pub width: Spring,
    pub height: Spring,
    pub corner_radius: Spring,
    pub opacity: Spring,
}

impl SpringRect {
    pub fn new(x: f32, y: f32, w: f32, h: f32, r: f32) -> Self {
        // High-responsiveness spring profile: stiffness ~240, damping ~24, mass 1.0
        let k = 240.0;
        let c = 24.0;
        let m = 1.0;
        Self {
            x: Spring::new(x, k, c, m),
            y: Spring::new(y, k, c, m),
            width: Spring::new(w, k, c, m),
            height: Spring::new(h, k, c, m),
            corner_radius: Spring::new(r, k, c, m),
            opacity: Spring::new(1.0, 300.0, 30.0, 1.0),
        }
    }

    pub fn set_target(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, opacity: f32) {
        self.x.set_target(x);
        self.y.set_target(y);
        self.width.set_target(w);
        self.height.set_target(h);
        self.corner_radius.set_target(r);
        self.opacity.set_target(opacity);
    }

    pub fn is_settled(&self) -> bool {
        self.x.is_settled()
            && self.y.is_settled()
            && self.width.is_settled()
            && self.height.is_settled()
            && self.corner_radius.is_settled()
            && self.opacity.is_settled()
    }

    pub fn update(&mut self, dt: f32) -> bool {
        let mx = self.x.update(dt);
        let my = self.y.update(dt);
        let mw = self.width.update(dt);
        let mh = self.height.update(dt);
        let mr = self.corner_radius.update(dt);
        let mo = self.opacity.update(dt);
        mx || my || mw || mh || mr || mo
    }
}

use forge3d::runtime::View;
use macroquad::prelude::*;

pub(super) struct Basis {
    pub right: Vec3,
    pub up: Vec3,
    pub back: Vec3,
}

pub(super) fn basis(view: &View) -> Basis {
    let (sin_yaw, cos_yaw) = view.yaw.to_radians().sin_cos();
    let (sin_pitch, cos_pitch) = view.pitch.clamp(-89., 89.).to_radians().sin_cos();
    let right = vec3(cos_yaw, 0., -sin_yaw);
    let back = vec3(sin_yaw * cos_pitch, sin_pitch, cos_yaw * cos_pitch);
    Basis {
        right,
        up: back.cross(right),
        back,
    }
}

pub(super) fn look(view: &mut View, dx: f32, dy: f32, orbit: bool) {
    let eye_offset = basis(view).back * view.distance.max(0.1);
    view.yaw = (view.yaw - dx * 0.35).rem_euclid(360.);
    view.pitch = (view.pitch + dy * 0.35).clamp(-89., 89.);
    if !orbit {
        let offset = eye_offset - basis(view).back * view.distance.max(0.1);
        view.target = (Vec3::from_array(view.target) + offset).to_array();
    }
}

pub(super) fn navigate(view: &mut View, is_2d: bool, input: Vec3, seconds: f32, fast: bool) {
    let input = input.clamp_length_max(1.);
    let units = view.distance.max(2.) * 0.75 * seconds.clamp(0., 0.05);
    let offset = if is_2d {
        vec3(input.x, -input.y, 0.)
    } else {
        let basis = basis(view);
        basis.right * input.x - basis.back * input.y + Vec3::Y * input.z
    };
    let speed = if fast { 3. } else { 1. };
    view.target =
        (Vec3::from_array(view.target) + offset.clamp_length_max(1.) * units * speed).to_array();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camera_moves_in_canvas_coordinates_and_accelerates() {
        let mut view = View {
            target: [3., 4., 5.],
            distance: 8.,
            ..View::default()
        };
        navigate(&mut view, true, Vec3::Y, 0.05, false);
        assert!((view.target[1] - 3.7).abs() < 1e-6);
        navigate(&mut view, true, Vec3::X, 0.05, true);
        assert!((view.target[0] - 3.9).abs() < 1e-6);
        assert_eq!(view.target[2], 5.);
    }

    #[test]
    fn forward_and_back_follow_view_pitch() {
        let mut view = View {
            target: [0.; 3],
            distance: 8.,
            yaw: 90.,
            pitch: 60.,
        };
        let original_eye = view.camera().position;
        let backwards = (original_eye - view.camera().target).normalize();
        navigate(&mut view, false, -Vec3::Y, 0.05, false);
        let offset = view.camera().position - original_eye;
        assert!(offset.y > 0.25);
        assert!(offset.distance(backwards * 0.3) < 1e-6);
        navigate(&mut view, false, Vec3::Y, 0.05, false);
        assert!(view.camera().position.distance(original_eye) < 1e-6);
        navigate(&mut view, false, Vec3::X, 0.05, false);
        assert!(Vec3::from_array(view.target).distance(vec3(0., 0., -0.3)) < 1e-6);
    }

    #[test]
    fn free_look_keeps_eye_fixed_and_orbit_keeps_target_fixed() {
        let mut view = View::default();
        let eye = view.camera().position;
        look(&mut view, 80., -65., false);
        assert!(view.camera().position.distance(eye) < 1e-6);
        let target = view.target;
        look(&mut view, 80., -65., true);
        assert_eq!(view.target, target);
        assert!(view.camera().position.distance(eye) > 0.1);
    }
}

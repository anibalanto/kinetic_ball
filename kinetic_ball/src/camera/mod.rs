mod follow;
mod split_screen;

pub use follow::{camera_follow_player_and_ball, camera_zoom_control};
pub use split_screen::{
    calculate_split_angle, update_split_screen_state, update_split_compositor, update_camera_viewports,
};

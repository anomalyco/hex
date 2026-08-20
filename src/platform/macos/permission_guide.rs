use objc2::MainThreadMarker;

unsafe extern "C" {
    fn hex_show_permission_guide(permission: i32);
}

fn show(permission: i32) {
    if MainThreadMarker::new().is_some() {
        unsafe { hex_show_permission_guide(permission) };
    }
}

pub(crate) fn show_input_monitoring() {
    show(0);
}

pub(crate) fn show_accessibility() {
    show(1);
}

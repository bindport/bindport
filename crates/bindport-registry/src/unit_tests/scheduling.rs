// SPDX-License-Identifier: MIT

use super::*;

#[test]
fn auto_render_reservations_apply_debounce_windows() {
    let mut registry =
        Registry::open(temp_registry_path("auto-render-reservation")).expect("registry");

    let first = registry
        .reserve_auto_render_at("traefik", 250, 1_000)
        .expect("first reservation");
    let second = registry
        .reserve_auto_render_at("traefik", 250, 1_100)
        .expect("debounced reservation");
    let disabled = registry
        .reserve_auto_render_at("traefik", 0, 1_100)
        .expect("disabled debounce");

    assert_eq!(first, Duration::from_millis(0));
    assert_eq!(second, Duration::from_millis(150));
    assert_eq!(disabled, Duration::from_millis(0));
}

// V module — temperature and unit conversions
// Built by build.rs with: v -shared -gc none -o <out>/libv_module v_module.v
// (a non-main module, so the shared library carries no `main` and links into Rust).

module v_module

@[export: 'v_celsius_to_fahrenheit']
pub fn v_celsius_to_fahrenheit(c f64) f64 {
	return c * 9.0 / 5.0 + 32.0
}

@[export: 'v_km_to_miles']
pub fn v_km_to_miles(km f64) f64 {
	return km * 0.621371
}

@[export: 'v_kelvin_to_rankine']
pub fn v_kelvin_to_rankine(k f64) f64 {
	return k * 1.8
}

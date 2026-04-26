#[no_mangle]
pub extern "C" fn process(in_l: f32, in_r: f32) -> i64 {
    // Simple Ring Modulator / Distortion
    let out_l = (in_l * 5.0).tanh();
    let out_r = (in_r * 5.0).tanh();

    let l_bits = out_l.to_bits() as u64;
    let r_bits = out_r.to_bits() as u64;

    ((l_bits << 32) | r_bits) as i64
}

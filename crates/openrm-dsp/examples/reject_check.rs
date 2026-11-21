use openrm_dsp::bandpass::{BandPassCore, Params};
fn main() {
    let fs = 48000.0;
    let mut p = Params::default();
    p.freq_l = 1000.0;
    p.bw_l = 1.0;
    p.reject_l = true;
    let mut core = BandPassCore::new();
    core.prepare(fs);
    core.set_params(&p);
    let n = fs as usize;
    let mut input = vec![0.0f32; n];
    let mut out = vec![0.0f32; n];
    for i in 0..n { input[i] = (2.0*std::f64::consts::PI*1000.0*i as f64/fs).sin() as f32; }
    let mut pos = 0;
    while pos < n {
        let b = 512.min(n - pos);
        core.update_smoothing(b);
        core.process_mono(&input[pos..pos+b], &mut out[pos..pos+b], None, b);
        pos += b;
    }
    let skip = (fs*0.1) as usize;
    let rms: f64 = out[skip..].iter().map(|x| (*x as f64)*(*x as f64)).sum::<f64>() / (n - skip) as f64;
    println!("reject center measured {:.1} dB", 20.0*(rms.sqrt()/(1.0/2f64.sqrt())).log10());
}

use std::fs::{File, OpenOptions};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;

mod common;
use common::{checked_path, invalid};
use trace_alloc::dump::{Counts, SampleStream};

const MAX_SAMPLES: usize = 1_000_000;

#[derive(Parser)]
#[command(
    name = "mtl",
    about = "Draw a memory timeline SVG from an allocator dump"
)]
struct Args {
    /// Allocator sample dump
    dump_path: PathBuf,
    /// SVG file to create
    #[arg(value_name = "OUTPUT.svg")]
    output_path: PathBuf,
}

fn draw_svg(mut out: impl Write, samples: &[Counts]) -> io::Result<()> {
    let maximum = samples
        .iter()
        .flat_map(|sample| {
            [
                sample.acc_bytes,
                sample.alive_bytes as u64,
                sample.peak_alive_bytes as u64,
            ]
        })
        .max()
        .unwrap_or(0)
        .max(1) as f64;
    let maximum_calls = samples
        .iter()
        .flat_map(|sample| [sample.realloc_growth_count, sample.realloc_shrink_count])
        .max()
        .unwrap_or(0)
        .max(1) as f64;
    let peak = samples
        .iter()
        .map(|sample| sample.peak_alive_bytes)
        .max()
        .unwrap_or(0);
    let peak_mib = peak as f64 / 1_048_576.0;
    writeln!(
        out,
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="100%" viewBox="0 0 1140 660" style="display:block; width:100%; height:auto;">
<title>Memory timeline byte and call counters</title>
<rect id="background" width="1140" height="660" fill="white"/>
<g font-family="sans-serif" font-size="14" fill="#222">
<text x="1120" y="635" text-anchor="end">peak: {peak_mib:.0} MiB</text>
<text x="90" y="605" fill="#eab308">acc</text>
<text x="190" y="605" fill="#16a34a">alive</text>
<text x="290" y="605" fill="#dc2626">peak</text>
<text x="800" y="605" fill="#ec4899">realloc_growth_call</text>
<text x="970" y="605" fill="#0891b2">realloc_shrink_call</text>"##
    )?;
    for tick in 0..=5 {
        let y = 500.0 - tick as f64 * 80.0;
        writeln!(
            out,
            r##"<path d="M90 {y} H960" stroke="#e5e7eb"/>
<text x="80" y="{}" text-anchor="end">{:.0}</text>
<text x="975" y="{}">{:.0}</text>"##,
            y + 5.0,
            maximum * tick as f64 / 5.0 / 1_048_576.0,
            y + 5.0,
            maximum_calls * tick as f64 / 5.0
        )?;
    }
    writeln!(
        out,
        r##"<path d="M90 100 V500 H960 V100" fill="none" stroke="#222"/>
<text x="90" y="90">MiB</text>
<text x="975" y="90">Calls</text>
<text x="90" y="525">1</text>
<text x="960" y="525" text-anchor="end">{}</text>
<text x="525" y="565" text-anchor="middle">Sample number</text>"##,
        samples.len()
    )?;
    for (column, color) in [
        (2, "#dc2626"),
        (3, "#ec4899"),
        (4, "#0891b2"),
        (0, "#eab308"),
        (1, "#16a34a"),
    ] {
        let scale = if column < 3 { maximum } else { maximum_calls };
        if column == 2 {
            let y = 500.0 - peak as f64 / scale * 400.0;
            writeln!(
                out,
                r#"<path d="M90 {y:.2} H960" fill="none" stroke="{color}" stroke-width="1" stroke-opacity="0.5"/>"#
            )?;
            continue;
        }
        let width = if column == 0 || column == 1 { 2 } else { 1 };
        write!(
            out,
            r#"<polyline fill="none" stroke="{color}" stroke-width="{width}" points=""#
        )?;
        for (index, sample) in samples.iter().enumerate() {
            let x = 90.0 + index as f64 / samples.len().saturating_sub(1).max(1) as f64 * 870.0;
            let y = 500.0 - counter(sample, column) as f64 / scale * 400.0;
            write!(out, "{x:.2},{y:.2} ")?;
        }
        writeln!(out, "\"/>")?;
        if samples.len() == 1 {
            let y = 500.0 - counter(&samples[0], column) as f64 / scale * 400.0;
            writeln!(out, r#"<circle cx="90" cy="{y:.2}" r="3" fill="{color}"/>"#)?;
        }
    }
    writeln!(
        out,
        r##"<g id="crosshair" visibility="hidden" pointer-events="none">
<path id="crosshair-lines" fill="none" stroke="#64748b" stroke-dasharray="4 4"/>
<g font-size="12" paint-order="stroke" stroke="white" stroke-width="5" stroke-linejoin="round">
<text id="crosshair-x" y="545" text-anchor="middle"/>
<text id="crosshair-y" x="98"/>
<text id="crosshair-calls" x="952" text-anchor="end"/>
</g>
</g>
<rect id="hover-area" x="90" y="100" width="870" height="400" fill="transparent" pointer-events="all" cursor="crosshair" data-samples="{}" data-maximum="{maximum}" data-maximum-calls="{maximum_calls}"/>
</g>"##,
        samples.len()
    )?;
    out.write_all(
        r#"<script><![CDATA[
(() => {
    const svg = document.currentScript.ownerSVGElement;
    const area = svg.querySelector('#hover-area');
    const crosshair = svg.querySelector('#crosshair');
    const lines = svg.querySelector('#crosshair-lines');
    const xLabel = svg.querySelector('#crosshair-x');
    const yLabel = svg.querySelector('#crosshair-y');
    const callsLabel = svg.querySelector('#crosshair-calls');
    const samples = Number(area.dataset.samples);
    const maximum = Number(area.dataset.maximum);
    const maximumCalls = Number(area.dataset.maximumCalls);
    if (!Number.isSafeInteger(samples) || samples < 1 || samples > 1000000 ||
        !Number.isFinite(maximum) || maximum <= 0 ||
        !Number.isFinite(maximumCalls) || maximumCalls <= 0) return;

    area.addEventListener('pointermove', event => {
        const matrix = svg.getScreenCTM();
        if (!matrix) return;
        const point = svg.createSVGPoint();
        point.x = event.clientX;
        point.y = event.clientY;
        const local = point.matrixTransform(matrix.inverse());
        if (!Number.isFinite(local.x) || !Number.isFinite(local.y)) return;
        const x = Math.max(90, Math.min(960, local.x));
        const y = Math.max(100, Math.min(500, local.y));
        const sample = 1 + (x - 90) / 870 * (samples - 1);
        const mib = (500 - y) / 400 * maximum / 1048576;
        lines.setAttribute('d', `M${x} 100 V500 M90 ${y} H960`);
        xLabel.setAttribute('x', Math.max(130, Math.min(920, x)));
        xLabel.textContent = `x: ${sample.toFixed(0)}`;
        yLabel.setAttribute('y', Math.max(115, y - 8));
        yLabel.textContent = `y: ${mib.toFixed(0)} MiB`;
        callsLabel.setAttribute('y', Math.max(115, y - 8));
        callsLabel.textContent = `${((500 - y) / 400 * maximumCalls).toFixed(0)} calls`;
        crosshair.setAttribute('visibility', 'visible');
    });
    area.addEventListener('pointerleave', () => {
        crosshair.setAttribute('visibility', 'hidden');
    });
})();
]]></script>
</svg>
"#
        .as_bytes(),
    )?;
    out.flush()
}

fn counter(sample: &Counts, column: usize) -> u64 {
    match column {
        0 => sample.acc_bytes,
        1 => sample.alive_bytes as u64,
        2 => sample.peak_alive_bytes as u64,
        3 => sample.realloc_growth_count,
        4 => sample.realloc_shrink_count,
        _ => unreachable!(),
    }
}

fn read_samples(file: File) -> io::Result<Vec<Counts>> {
    let mut bytes = BufReader::new(file).bytes();
    let mut read_error = None;
    let mut stream = std::iter::from_fn(|| match bytes.next()? {
        Ok(byte) => Some(byte),
        Err(error) => {
            read_error = Some(error);
            None
        }
    });
    let mut samples = Vec::new();
    for sample in SampleStream::new(&mut stream) {
        match sample {
            Ok(sample) => {
                if samples.len() == MAX_SAMPLES {
                    return Err(invalid("dump exceeds the 1,000,000 sample limit"));
                }
                samples.push(sample.counts);
            }
            Err(range) => {
                return Err(invalid(format!(
                    "malformed sample at bytes {}..{}",
                    range.start, range.end,
                )));
            }
        }
    }
    if let Some(error) = read_error {
        return Err(error);
    }
    if samples.is_empty() {
        return Err(invalid("dump contains no valid samples"));
    }
    Ok(samples)
}

fn run() -> io::Result<()> {
    let args = Args::parse();
    let input = checked_path(&args.dump_path)?;
    let output = checked_path(&args.output_path)?;
    let file = File::open(input)?;
    if !file.metadata()?.is_file() {
        return Err(invalid("input must be a regular file"));
    }
    let samples = read_samples(file)?;
    let output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)?;
    draw_svg(BufWriter::new(output), &samples)
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("mtl: {error}");
            ExitCode::FAILURE
        }
    }
}

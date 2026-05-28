use layout::layout;

#[inline(never)]
fn print_rect(x: f32, y: f32, width: f32, height: f32) {
    println!("x={x} y={y} width={width} height={height}");
}

fn main() {
    layout!(
        {
            available_width: 800,
            available_height: 600,
            direction: column,
            padding_top: 10,
            padding_left: 20,
            gap: 8,

            layout!({ width: 760, height: 100 }, { print_rect(x, y, width, height); }),
            layout!({ width: 760, height: 200 }, { print_rect(x, y, width, height); }),
        },
        { print_rect(x, y, width, height); }
    );
}

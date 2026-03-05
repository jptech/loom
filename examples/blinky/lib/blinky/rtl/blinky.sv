// LED blinker — toggles LEDs at visible rate using a counter
module blinky #(
    parameter int CLK_FREQ_HZ = 100_000_000,
    parameter int NUM_LEDS    = 4
) (
    input  logic                clk,
    input  logic                rst_n,
    output logic [NUM_LEDS-1:0] led
);

    // Counter width: enough bits so MSBs toggle at ~1-4 Hz
    localparam int CNT_WIDTH = $clog2(CLK_FREQ_HZ) + NUM_LEDS;

    logic [CNT_WIDTH-1:0] cnt;

    counter #(
        .WIDTH(CNT_WIDTH)
    ) u_counter (
        .clk   (clk),
        .rst_n (rst_n),
        .en    (1'b1),
        .count (cnt)
    );

    // Drive LEDs from the top bits of the counter
    assign led = cnt[CNT_WIDTH-1 -: NUM_LEDS];

endmodule

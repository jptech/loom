// Top-level module for Basys 3 blinky demo
module top (
    input  logic       clk,     // 100 MHz board clock (W5)
    input  logic       rst_n,   // Active-low reset (active-low from button, directly usable)
    output logic [3:0] led      // LEDs LD0-LD3
);

    blinky #(
        .CLK_FREQ_HZ(100_000_000),
        .NUM_LEDS   (4)
    ) u_blinky (
        .clk   (clk),
        .rst_n (rst_n),
        .led   (led)
    );

endmodule

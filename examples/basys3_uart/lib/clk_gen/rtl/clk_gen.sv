// MMCME2_BASE wrapper: 100 MHz input -> 50 MHz output
// Xilinx 7-series specific primitive instantiation
module clk_gen (
    input  logic clk_in,      // 100 MHz board clock
    input  logic rst,          // active-high reset
    output logic clk_out,      // 50 MHz generated clock
    output logic locked        // MMCM lock indicator
);

    logic clk_fb;
    logic clk_out_unbuf;

    MMCME2_BASE #(
        .BANDWIDTH         ("OPTIMIZED"),
        .CLKIN1_PERIOD     (10.0),       // 100 MHz input
        .CLKFBOUT_MULT_F   (10.0),       // VCO = 100 * 10 = 1000 MHz
        .CLKOUT0_DIVIDE_F  (20.0),       // 1000 / 20 = 50 MHz
        .CLKOUT0_DUTY_CYCLE(0.5),
        .CLKOUT0_PHASE     (0.0),
        .DIVCLK_DIVIDE     (1),
        .REF_JITTER1       (0.010),
        .STARTUP_WAIT      ("FALSE")
    ) mmcme2_inst (
        .CLKIN1   (clk_in),
        .CLKFBIN  (clk_fb),
        .RST      (rst),
        .PWRDWN   (1'b0),

        .CLKOUT0  (clk_out_unbuf),
        .CLKOUT0B (),
        .CLKOUT1  (),
        .CLKOUT1B (),
        .CLKOUT2  (),
        .CLKOUT2B (),
        .CLKOUT3  (),
        .CLKOUT3B (),
        .CLKOUT4  (),
        .CLKOUT5  (),
        .CLKOUT6  (),
        .CLKFBOUT (clk_fb),
        .CLKFBOUTB(),
        .LOCKED   (locked)
    );

    BUFG bufg_clk50 (
        .I (clk_out_unbuf),
        .O (clk_out)
    );

endmodule

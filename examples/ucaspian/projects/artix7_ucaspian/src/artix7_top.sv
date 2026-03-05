// artix7_top.sv — uCaspian on Numato Mimas A7 (Xilinx Artix-7)
//
// Originally from uCaspian (https://github.com/ornl/ucaspian)
// Copyright (c) 2025 UT-BATTELLE, LLC — MIT License
// Adapted for Loom FPGA build system example
//
// Build Tools: Vivado
// Board: Numato Mimas A7 (XC7A50T-1FGG484C)
// Comm Interface: USB-UART bridge
//
// This is a minimal adaptation of the ice40 top-level:
//   - Replaces SB_HFOSC with MMCME2_BASE + BUFG for clock generation
//   - 100 MHz board clock -> 24 MHz system clock (preserves UART baud math)
//   - No LED logic — clock + UART + core only

module artix7_top (
    input  logic clk,              // 100 MHz board oscillator
    input  logic reset,            // Active-high reset (active-high push button)
    input  logic usb_uart_rxd,     // UART RX from USB bridge
    output logic usb_uart_txd      // UART TX to USB bridge
);

    // ----------------------------------------------------------------
    // Clock generation: 100 MHz -> 24 MHz via MMCM
    // VCO = 100 * 12 = 1200 MHz, CLKOUT0 = 1200 / 50 = 24 MHz
    // ----------------------------------------------------------------
    logic clk_sys;
    logic clk_fb;
    logic clk_24m_unbuf;
    logic mmcm_locked;

    MMCME2_BASE #(
        .BANDWIDTH         ("OPTIMIZED"),
        .CLKIN1_PERIOD     (10.0),       // 100 MHz input
        .CLKFBOUT_MULT_F   (12.0),       // VCO = 100 * 12 = 1200 MHz
        .CLKOUT0_DIVIDE_F  (50.0),       // 1200 / 50 = 24 MHz
        .CLKOUT0_DUTY_CYCLE(0.5),
        .CLKOUT0_PHASE     (0.0),
        .DIVCLK_DIVIDE     (1),
        .REF_JITTER1       (0.010),
        .STARTUP_WAIT      ("FALSE")
    ) mmcm_inst (
        .CLKIN1   (clk),
        .CLKFBIN  (clk_fb),
        .RST      (reset),
        .PWRDWN   (1'b0),

        .CLKOUT0  (clk_24m_unbuf),
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
        .LOCKED   (mmcm_locked)
    );

    BUFG bufg_clk24 (
        .I (clk_24m_unbuf),
        .O (clk_sys)
    );

    // ----------------------------------------------------------------
    // Synchronous reset: hold reset until MMCM locks
    // ----------------------------------------------------------------
    logic sys_reset;
    logic [3:0] reset_pipe;

    always_ff @(posedge clk_sys or negedge mmcm_locked) begin
        if (!mmcm_locked)
            reset_pipe <= 4'hF;
        else
            reset_pipe <= {reset_pipe[2:0], 1'b0};
    end

    always_comb sys_reset = reset_pipe[3];

    // ----------------------------------------------------------------
    // Baud clocks: 24 MHz -> 3 MHz (x1) and 12 MHz (x4)
    // ----------------------------------------------------------------
    logic clk_1;
    logic clk_4;

    divide_by_n #(.N(8)) div1(clk_sys, sys_reset, clk_1);
    divide_by_n #(.N(2)) div4(clk_sys, sys_reset, clk_4);

    // ----------------------------------------------------------------
    // UART FIFOs
    // ----------------------------------------------------------------
    logic [7:0] read_data;
    logic read_rdy, read_vld;
    logic read_fifo_enable;
    logic read_fifo_empty;

    logic [7:0] write_data;
    logic write_rdy, write_vld;
    logic write_fifo_enable;
    logic write_fifo_full;

    uart_tx_fifo #(
        .FIFO_DEPTH(1024)
    ) uart_fifo_outgoing (
        .clk(clk_sys),
        .reset(sys_reset),
        .baud_x1(clk_1),
        .data(write_data),
        .write_enable(write_fifo_enable),
        .rts(1'b0),
        .fifo_full(write_fifo_full),
        .fifo_empty(),
        .fifo_almost_full(),
        .serial(usb_uart_txd)
    );
    always_comb write_fifo_enable = write_rdy & write_vld;
    always_comb write_rdy = ~write_fifo_full;

    uart_rx_fifo #(
        .FIFO_DEPTH(1024)
    ) uart_fifo_incoming (
        .clk(clk_sys),
        .reset(sys_reset),
        .baud_x4(clk_4),
        .read_enable(read_fifo_enable),
        .data(read_data),
        .fifo_full(),
        .fifo_empty(read_fifo_empty),
        .cts(),
        .serial(usb_uart_rxd)
    );
    always_comb read_fifo_enable = read_rdy & read_vld;
    always_comb read_vld = ~read_fifo_empty;

    // ----------------------------------------------------------------
    // uCaspian core
    // ----------------------------------------------------------------
    ucaspian ucaspian_inst (
        .sys_clk(clk_sys),
        .reset(sys_reset),

        .read_data(read_data),
        .read_vld(read_vld),
        .read_rdy(read_rdy),

        .write_data(write_data),
        .write_vld(write_vld),
        .write_rdy(write_rdy),

        .led_0(),
        .led_1(),
        .led_2(),
        .led_3()
    );

endmodule

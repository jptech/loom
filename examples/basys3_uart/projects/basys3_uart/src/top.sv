// UART echo with 7-segment display — Basys 3 top level
// 100 MHz board clock, 50 MHz UART clock via MMCM, CDC between domains
module top (
    input  logic       clk,            // 100 MHz board oscillator
    input  logic       rst_n,          // active-low reset (center button)
    input  logic       uart_rxd,       // USB-UART RX
    output logic       uart_txd,       // USB-UART TX
    output logic [6:0] seg,            // 7-segment cathodes
    output logic [3:0] an,             // 7-segment anodes
    output logic       led_heartbeat   // heartbeat LED
);

    // ── Clock generation ───────────────────────────────────────────────
    logic clk_50m;
    logic mmcm_locked;

    clk_gen u_clk_gen (
        .clk_in  (clk),
        .rst     (~rst_n),
        .clk_out (clk_50m),
        .locked  (mmcm_locked)
    );

    // Synchronous reset for 50 MHz domain
    logic rst_50m_n;
    always_ff @(posedge clk_50m or negedge rst_n) begin
        if (!rst_n)
            rst_50m_n <= 1'b0;
        else
            rst_50m_n <= mmcm_locked;
    end

    // ── UART (50 MHz domain) ───────────────────────────────────────────
    logic [7:0] rx_data;
    logic       rx_valid;

    uart_rx #(
        .CLK_FREQ (50_000_000),
        .BAUD     (115200)
    ) u_uart_rx (
        .clk      (clk_50m),
        .rst_n    (rst_50m_n),
        .rx       (uart_rxd),
        .rx_data  (rx_data),
        .rx_valid (rx_valid)
    );

    // Echo: connect RX data directly to TX
    logic       tx_ready;

    uart_tx #(
        .CLK_FREQ (50_000_000),
        .BAUD     (115200)
    ) u_uart_tx (
        .clk      (clk_50m),
        .rst_n    (rst_50m_n),
        .tx_data  (rx_data),
        .tx_valid (rx_valid),
        .tx       (uart_txd),
        .tx_ready (tx_ready)
    );

    // ── CDC: rx_valid pulse from 50 MHz → 100 MHz ─────────────────────
    logic rx_valid_sync;

    cdc_sync #(
        .WIDTH(1)
    ) u_cdc_rx_valid (
        .clk   (clk),
        .rst_n (rst_n),
        .d     (rx_valid),
        .q     (rx_valid_sync)
    );

    // Latch received byte in 100 MHz domain for display
    logic [7:0] rx_data_latched;

    always_ff @(posedge clk or negedge rst_n) begin
        if (!rst_n)
            rx_data_latched <= 8'h00;
        else if (rx_valid_sync)
            rx_data_latched <= rx_data;
    end

    // ── 7-Segment display (100 MHz domain) ────────────────────────────
    // Show last received byte as two hex digits on rightmost two displays
    // Left two digits show 0x00
    seg7_controller u_seg7 (
        .clk    (clk),
        .rst_n  (rst_n),
        .digit3 (4'h0),
        .digit2 (4'h0),
        .digit1 (rx_data_latched[7:4]),
        .digit0 (rx_data_latched[3:0]),
        .seg    (seg),
        .an     (an)
    );

    // ── Heartbeat LED (100 MHz domain) ────────────────────────────────
    logic [25:0] heartbeat_cnt;

    counter #(
        .WIDTH(26)
    ) u_heartbeat (
        .clk   (clk),
        .rst_n (rst_n),
        .en    (1'b1),
        .count (heartbeat_cnt)
    );

    assign led_heartbeat = heartbeat_cnt[25];  // ~1.5 Hz blink

endmodule

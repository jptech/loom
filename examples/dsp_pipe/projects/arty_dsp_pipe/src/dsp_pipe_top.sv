// dsp_pipe_top.sv — Top-level DSP signal processing pipeline
//
// Pipeline chain: gain_ctrl → window_func → fir_filter → output
// Each stage connected via AXI-Stream with skid buffers for decoupling.

module dsp_pipe_top #(
    parameter int DATA_WIDTH    = 16,
    parameter int COEFF_WIDTH   = 16,
    parameter int FILTER_ORDER  = 32,
    parameter int GAIN_WIDTH    = 8,
    parameter int GAIN_FRAC     = 4,
    parameter int WINDOW_SIZE   = 256,
    parameter     COEFF_FILE    = "fir_coeffs.mem",
    parameter     WINDOW_FILE   = "window_lut.mem"
) (
    input  logic                   clk,
    input  logic                   rst_n,

    // Data input (AXI-Stream)
    input  logic [DATA_WIDTH-1:0]  data_in,
    input  logic                   data_in_valid,
    output logic                   data_in_ready,

    // Data output (AXI-Stream)
    output logic [DATA_WIDTH-1:0]  data_out,
    output logic                   data_out_valid,
    input  logic                   data_out_ready,

    // Gain control
    input  logic [GAIN_WIDTH-1:0]  gain,

    // Status LEDs
    output logic [3:0]             led
);

    // --- Stage 1: Gain Control ---
    logic [DATA_WIDTH-1:0] gain_out_data;
    logic                  gain_out_valid;
    logic                  gain_out_ready;
    logic                  gain_saturated;

    gain_ctrl #(
        .DATA_WIDTH (DATA_WIDTH),
        .GAIN_WIDTH (GAIN_WIDTH),
        .GAIN_FRAC  (GAIN_FRAC)
    ) u_gain (
        .clk       (clk),
        .rst_n     (rst_n),
        .gain      (gain),
        .s_tdata   (data_in),
        .s_tvalid  (data_in_valid),
        .s_tready  (data_in_ready),
        .m_tdata   (gain_out_data),
        .m_tvalid  (gain_out_valid),
        .m_tready  (gain_out_ready),
        .saturated (gain_saturated)
    );

    // --- Skid buffer 1: Gain → Window ---
    logic [DATA_WIDTH-1:0] skid1_out_data;
    logic                  skid1_out_valid;
    logic                  skid1_out_ready;

    pipeline_skid_buffer #(
        .WIDTH(DATA_WIDTH)
    ) u_skid1 (
        .clk      (clk),
        .rst_n    (rst_n),
        .s_tdata  (gain_out_data),
        .s_tvalid (gain_out_valid),
        .s_tready (gain_out_ready),
        .m_tdata  (skid1_out_data),
        .m_tvalid (skid1_out_valid),
        .m_tready (skid1_out_ready)
    );

    // --- Stage 2: Window Function ---
    logic [DATA_WIDTH-1:0] win_out_data;
    logic                  win_out_valid;
    logic                  win_out_ready;

    window_func #(
        .DATA_WIDTH (DATA_WIDTH),
        .LUT_SIZE   (WINDOW_SIZE),
        .LUT_WIDTH  (DATA_WIDTH),
        .LUT_FILE   (WINDOW_FILE)
    ) u_window (
        .clk      (clk),
        .rst_n    (rst_n),
        .s_tdata  (skid1_out_data),
        .s_tvalid (skid1_out_valid),
        .s_tready (skid1_out_ready),
        .m_tdata  (win_out_data),
        .m_tvalid (win_out_valid),
        .m_tready (win_out_ready)
    );

    // --- Skid buffer 2: Window → FIR ---
    logic [DATA_WIDTH-1:0] skid2_out_data;
    logic                  skid2_out_valid;
    logic                  skid2_out_ready;

    pipeline_skid_buffer #(
        .WIDTH(DATA_WIDTH)
    ) u_skid2 (
        .clk      (clk),
        .rst_n    (rst_n),
        .s_tdata  (win_out_data),
        .s_tvalid (win_out_valid),
        .s_tready (win_out_ready),
        .m_tdata  (skid2_out_data),
        .m_tvalid (skid2_out_valid),
        .m_tready (skid2_out_ready)
    );

    // --- Stage 3: FIR Filter ---
    fir_filter #(
        .DATA_WIDTH  (DATA_WIDTH),
        .COEFF_WIDTH (COEFF_WIDTH),
        .ORDER       (FILTER_ORDER),
        .COEFF_FILE  (COEFF_FILE)
    ) u_fir (
        .clk      (clk),
        .rst_n    (rst_n),
        .s_tdata  (skid2_out_data),
        .s_tvalid (skid2_out_valid),
        .s_tready (skid2_out_ready),
        .m_tdata  (data_out),
        .m_tvalid (data_out_valid),
        .m_tready (data_out_ready)
    );

    // --- Status LEDs ---
    assign led[0] = data_in_valid;
    assign led[1] = data_out_valid;
    assign led[2] = gain_saturated;
    assign led[3] = 1'b0;

endmodule

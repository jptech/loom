// window_func.sv — Windowing function via ROM lookup table
//
// Multiplies input samples by a precomputed window function stored in ROM.
// The window LUT is loaded from a .mem file via $readmemh.

module window_func #(
    parameter int DATA_WIDTH  = 16,
    parameter int LUT_SIZE    = 256,
    parameter int LUT_WIDTH   = 16,
    parameter     LUT_FILE    = "window_lut.mem"
) (
    input  logic                   clk,
    input  logic                   rst_n,

    // AXI-Stream input
    input  logic [DATA_WIDTH-1:0]  s_tdata,
    input  logic                   s_tvalid,
    output logic                   s_tready,

    // AXI-Stream output
    output logic [DATA_WIDTH-1:0]  m_tdata,
    output logic                   m_tvalid,
    input  logic                   m_tready
);

    localparam int IDX_WIDTH = $clog2(LUT_SIZE);

    // Window coefficient ROM
    logic [LUT_WIDTH-1:0] window_lut [0:LUT_SIZE-1];
    initial $readmemh(LUT_FILE, window_lut);

    // Sample index counter (wraps at LUT_SIZE)
    logic [IDX_WIDTH-1:0] sample_idx;

    // Pipeline registers
    logic signed [DATA_WIDTH+LUT_WIDTH-1:0] product;
    logic                                    pipe_valid;

    assign s_tready = m_tready || !pipe_valid;

    always_ff @(posedge clk or negedge rst_n) begin
        if (!rst_n) begin
            sample_idx <= '0;
            product    <= '0;
            pipe_valid <= 1'b0;
        end else if (s_tvalid && s_tready) begin
            product    <= $signed(s_tdata) * $signed({1'b0, window_lut[sample_idx]});
            pipe_valid <= 1'b1;
            sample_idx <= (sample_idx == IDX_WIDTH'(LUT_SIZE - 1)) ? '0 : sample_idx + 1;
        end else if (m_tready) begin
            pipe_valid <= 1'b0;
        end
    end

    // Truncate product to output width (take MSBs)
    assign m_tdata  = product[DATA_WIDTH+LUT_WIDTH-2 -: DATA_WIDTH];
    assign m_tvalid = pipe_valid;

endmodule

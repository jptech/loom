// fir_filter.sv — Transposed-form FIR filter with AXI-Stream interface
//
// Coefficients loaded from .mem file via $readmemh at elaboration time.
// Parameterized order (number of taps) and data width.

module fir_filter #(
    parameter int DATA_WIDTH  = 16,
    parameter int COEFF_WIDTH = 16,
    parameter int ORDER       = 32,
    parameter     COEFF_FILE  = "fir_coeffs.mem"
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

    // Coefficient ROM
    logic [COEFF_WIDTH-1:0] coeffs [0:ORDER-1];
    initial $readmemh(COEFF_FILE, coeffs);

    // Delay line
    logic [DATA_WIDTH-1:0] delay_line [0:ORDER-1];

    // Accumulator chain (transposed form)
    logic signed [DATA_WIDTH+COEFF_WIDTH:0] acc [0:ORDER];

    integer i;

    // Accept data when ready
    assign s_tready = m_tready || !m_tvalid;

    always_ff @(posedge clk or negedge rst_n) begin
        if (!rst_n) begin
            for (i = 0; i < ORDER; i++) begin
                delay_line[i] <= '0;
            end
            m_tvalid <= 1'b0;
            m_tdata  <= '0;
        end else if (s_tvalid && s_tready) begin
            // Shift delay line
            delay_line[0] <= s_tdata;
            for (i = 1; i < ORDER; i++) begin
                delay_line[i] <= delay_line[i-1];
            end

            m_tvalid <= 1'b1;
            // Truncate accumulator to output width
            m_tdata <= acc[0][DATA_WIDTH+COEFF_WIDTH-1 -: DATA_WIDTH];
        end else if (m_tready) begin
            m_tvalid <= 1'b0;
        end
    end

    // Transposed-form accumulator (combinational for synthesis)
    always_comb begin
        acc[ORDER] = '0;
        for (i = ORDER - 1; i >= 0; i--) begin
            acc[i] = acc[i+1] + ($signed(delay_line[i]) * $signed(coeffs[i]));
        end
    end

endmodule

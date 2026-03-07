// gain_ctrl.sv — Programmable gain control with saturation
//
// Multiplies input by a configurable gain value. Saturates on overflow
// rather than wrapping. AXI-Stream interface.

module gain_ctrl #(
    parameter int DATA_WIDTH = 16,
    parameter int GAIN_WIDTH = 8,
    parameter int GAIN_FRAC  = 4   // fractional bits in gain (gain of 1.0 = 1<<GAIN_FRAC)
) (
    input  logic                    clk,
    input  logic                    rst_n,

    // Gain setting (unsigned fixed-point)
    input  logic [GAIN_WIDTH-1:0]   gain,

    // AXI-Stream input
    input  logic [DATA_WIDTH-1:0]   s_tdata,
    input  logic                    s_tvalid,
    output logic                    s_tready,

    // AXI-Stream output
    output logic [DATA_WIDTH-1:0]   m_tdata,
    output logic                    m_tvalid,
    input  logic                    m_tready,

    // Status
    output logic                    saturated
);

    localparam int PROD_WIDTH = DATA_WIDTH + GAIN_WIDTH;
    localparam int MAX_POS = (1 << (DATA_WIDTH - 1)) - 1;
    localparam int MIN_NEG = -(1 << (DATA_WIDTH - 1));

    logic signed [PROD_WIDTH-1:0] product;
    logic signed [PROD_WIDTH-1:0] scaled;
    logic                         pipe_valid;
    logic                         sat_flag;

    assign s_tready = m_tready || !pipe_valid;

    always_ff @(posedge clk or negedge rst_n) begin
        if (!rst_n) begin
            product    <= '0;
            pipe_valid <= 1'b0;
            sat_flag   <= 1'b0;
        end else if (s_tvalid && s_tready) begin
            product    <= $signed(s_tdata) * $signed({1'b0, gain});
            pipe_valid <= 1'b1;

            // Check for saturation after gain shift
            scaled = ($signed(s_tdata) * $signed({1'b0, gain})) >>> GAIN_FRAC;
            if (scaled > PROD_WIDTH'(MAX_POS))
                sat_flag <= 1'b1;
            else if (scaled < PROD_WIDTH'(signed'(MIN_NEG)))
                sat_flag <= 1'b1;
            else
                sat_flag <= 1'b0;
        end else if (m_tready) begin
            pipe_valid <= 1'b0;
        end
    end

    // Apply gain shift and saturate
    logic signed [PROD_WIDTH-1:0] shifted;
    assign shifted = product >>> GAIN_FRAC;

    always_comb begin
        if (shifted > PROD_WIDTH'(MAX_POS))
            m_tdata = DATA_WIDTH'(MAX_POS);
        else if (shifted < PROD_WIDTH'(signed'(MIN_NEG)))
            m_tdata = DATA_WIDTH'(MIN_NEG);
        else
            m_tdata = shifted[DATA_WIDTH-1:0];
    end

    assign m_tvalid  = pipe_valid;
    assign saturated = sat_flag;

endmodule

// fir_mac.sv — Single multiply-accumulate unit for FIR filter
//
// DSP48-friendly: registered multiply followed by registered add.
// Parameterized data and coefficient widths.

module fir_mac #(
    parameter int DATA_WIDTH  = 16,
    parameter int COEFF_WIDTH = 16,
    parameter int ACC_WIDTH   = 40
) (
    input  logic                    clk,
    input  logic                    rst_n,
    input  logic                    en,

    input  logic [DATA_WIDTH-1:0]   data_in,
    input  logic [COEFF_WIDTH-1:0]  coeff,
    input  logic [ACC_WIDTH-1:0]    acc_in,
    output logic [ACC_WIDTH-1:0]    acc_out
);

    // Registered multiply
    logic signed [DATA_WIDTH+COEFF_WIDTH-1:0] product;

    always_ff @(posedge clk or negedge rst_n) begin
        if (!rst_n) begin
            product <= '0;
            acc_out <= '0;
        end else if (en) begin
            product <= $signed(data_in) * $signed(coeff);
            acc_out <= acc_in + ACC_WIDTH'(product);
        end
    end

endmodule

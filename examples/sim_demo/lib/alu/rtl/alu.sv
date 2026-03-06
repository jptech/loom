// Simple ALU with arithmetic and logic operations
//
// Combinational datapath with registered output. Supports 6 operations
// selected by a 3-bit opcode. Zero flag is asserted when result is 0.
`timescale 1ns / 1ps

module alu #(
    parameter int WIDTH = 8
) (
    input  logic             clk,
    input  logic             rst_n,
    input  logic [WIDTH-1:0] a,
    input  logic [WIDTH-1:0] b,
    input  logic [2:0]       op,
    output logic [WIDTH-1:0] result,
    output logic             zero_flag
);

    // Operation encoding
    localparam logic [2:0] OP_ADD = 3'b000;
    localparam logic [2:0] OP_SUB = 3'b001;
    localparam logic [2:0] OP_AND = 3'b010;
    localparam logic [2:0] OP_OR  = 3'b011;
    localparam logic [2:0] OP_XOR = 3'b100;
    localparam logic [2:0] OP_NOT = 3'b101;

    logic [WIDTH-1:0] alu_out;

    // Combinational datapath
    always_comb begin
        case (op)
            OP_ADD:  alu_out = a + b;
            OP_SUB:  alu_out = a - b;
            OP_AND:  alu_out = a & b;
            OP_OR:   alu_out = a | b;
            OP_XOR:  alu_out = a ^ b;
            OP_NOT:  alu_out = ~a;
            default: alu_out = '0;
        endcase
    end

    // Registered output
    always_ff @(posedge clk or negedge rst_n) begin
        if (!rst_n) begin
            result    <= '0;
            zero_flag <= 1'b1;
        end else begin
            result    <= alu_out;
            zero_flag <= (alu_out == '0);
        end
    end

endmodule

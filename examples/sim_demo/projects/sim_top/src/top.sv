// Minimal integration top for sim_demo
//
// Instantiates the FIFO and ALU side by side.  ALU result feeds into
// the FIFO write-data port so there is a real datapath connection.
// This module exists so `loom build` has a valid top-level to
// elaborate; the individual testbenches drive the DUTs directly.
`timescale 1ns / 1ps
module top (
    input  logic       clk,
    input  logic       rst_n,
    // ALU interface
    input  logic [7:0] a,
    input  logic [7:0] b,
    input  logic [2:0] op,
    // FIFO control
    input  logic       fifo_wr_en,
    input  logic       fifo_rd_en,
    // Outputs
    output logic [7:0] fifo_rd_data,
    output logic       fifo_full,
    output logic       fifo_empty,
    output logic       alu_zero
);

    logic [7:0] alu_result;

    alu #(.WIDTH(8)) u_alu (
        .clk       (clk),
        .rst_n     (rst_n),
        .a         (a),
        .b         (b),
        .op        (op),
        .result    (alu_result),
        .zero_flag (alu_zero)
    );

    sync_fifo #(.WIDTH(8), .DEPTH(16)) u_fifo (
        .clk     (clk),
        .rst_n   (rst_n),
        .wr_en   (fifo_wr_en),
        .wr_data (alu_result),  // ALU feeds FIFO
        .rd_en   (fifo_rd_en),
        .rd_data (fifo_rd_data),
        .full    (fifo_full),
        .empty   (fifo_empty),
        .count   ()             // unused at top level
    );

endmodule

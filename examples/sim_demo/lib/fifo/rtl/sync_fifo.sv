// Synchronous FIFO with configurable width and depth
//
// Uses a circular buffer with separate read/write pointers and an
// explicit count register for flag generation. Designed for single
// clock domain use (for CDC, wrap with async handshake logic).
`timescale 1ns / 1ps

module sync_fifo #(
    parameter int WIDTH = 8,
    parameter int DEPTH = 16,
    parameter int ADDR_W = $clog2(DEPTH)
) (
    input  logic             clk,
    input  logic             rst_n,
    // Write port
    input  logic             wr_en,
    input  logic [WIDTH-1:0] wr_data,
    // Read port
    input  logic             rd_en,
    output logic [WIDTH-1:0] rd_data,
    // Status
    output logic             full,
    output logic             empty,
    output logic [ADDR_W:0]  count
);

    // Storage
    logic [WIDTH-1:0] mem [0:DEPTH-1];

    // Pointers
    logic [ADDR_W-1:0] wr_ptr;
    logic [ADDR_W-1:0] rd_ptr;

    // Internal write/read enable (gated by flags)
    wire do_write = wr_en && !full;
    wire do_read  = rd_en && !empty;

    // Flags
    assign full  = (count == DEPTH[ADDR_W:0]);
    assign empty = (count == '0);

    // Write logic
    always_ff @(posedge clk or negedge rst_n) begin
        if (!rst_n) begin
            wr_ptr <= '0;
        end else if (do_write) begin
            mem[wr_ptr] <= wr_data;
            wr_ptr      <= (wr_ptr == ADDR_W'(DEPTH - 1)) ? '0 : wr_ptr + 1'b1;
        end
    end

    // Read logic
    always_ff @(posedge clk or negedge rst_n) begin
        if (!rst_n) begin
            rd_ptr  <= '0;
            rd_data <= '0;
        end else if (do_read) begin
            rd_data <= mem[rd_ptr];
            rd_ptr  <= (rd_ptr == ADDR_W'(DEPTH - 1)) ? '0 : rd_ptr + 1'b1;
        end
    end

    // Count tracking
    always_ff @(posedge clk or negedge rst_n) begin
        if (!rst_n) begin
            count <= '0;
        end else begin
            case ({do_write, do_read})
                2'b10:   count <= count + 1'b1;
                2'b01:   count <= count - 1'b1;
                default: count <= count; // simultaneous or neither
            endcase
        end
    end

endmodule

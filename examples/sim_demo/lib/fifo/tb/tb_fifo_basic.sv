// Basic FIFO smoke test — write 4 values, read them back, verify ordering
//
// Self-checking testbench with $display pass/fail reporting.
// Tagged [smoke] in component.toml.
`timescale 1ns / 1ps

module tb_fifo_basic;

    parameter WIDTH = 8;
    parameter DEPTH = 16;

    logic             clk;
    logic             rst_n;
    logic             wr_en;
    logic [WIDTH-1:0] wr_data;
    logic             rd_en;
    logic [WIDTH-1:0] rd_data;
    logic             full;
    logic             empty;
    logic [$clog2(DEPTH):0] count;

    sync_fifo #(.WIDTH(WIDTH), .DEPTH(DEPTH)) dut (.*);

    // Clock generation — 10 ns period
    initial clk = 0;
    always #5 clk = ~clk;

    integer errors;

    task automatic write_one(input logic [WIDTH-1:0] data);
        @(posedge clk);
        wr_en   = 1'b1;
        wr_data = data;
        @(posedge clk);
        wr_en = 1'b0;
    endtask

    task automatic read_one(output logic [WIDTH-1:0] data);
        @(posedge clk);
        rd_en = 1'b1;
        @(posedge clk);
        rd_en = 1'b0;
        @(posedge clk);  // wait for registered rd_data to update
        data = rd_data;
    endtask

    logic [WIDTH-1:0] readback;

    initial begin
        errors = 0;
        rst_n  = 1'b0;
        wr_en  = 1'b0;
        rd_en  = 1'b0;
        wr_data = '0;

        // Reset
        repeat (3) @(posedge clk);
        rst_n = 1'b1;
        @(posedge clk);

        // Verify initial state
        if (!empty) begin
            $display("FAIL: FIFO not empty after reset");
            errors++;
        end
        if (full) begin
            $display("FAIL: FIFO full after reset");
            errors++;
        end

        // Write 4 values
        write_one(8'hAA);
        write_one(8'hBB);
        write_one(8'hCC);
        write_one(8'hDD);

        // Check count
        @(posedge clk);
        if (count !== 4) begin
            $display("FAIL: Expected count=4, got %0d", count);
            errors++;
        end

        // Read back and verify FIFO ordering
        read_one(readback);
        if (readback !== 8'hAA) begin
            $display("FAIL: Expected AA, got %02X", readback);
            errors++;
        end

        read_one(readback);
        if (readback !== 8'hBB) begin
            $display("FAIL: Expected BB, got %02X", readback);
            errors++;
        end

        read_one(readback);
        if (readback !== 8'hCC) begin
            $display("FAIL: Expected CC, got %02X", readback);
            errors++;
        end

        read_one(readback);
        if (readback !== 8'hDD) begin
            $display("FAIL: Expected DD, got %02X", readback);
            errors++;
        end

        // Verify empty after draining
        @(posedge clk);
        if (!empty) begin
            $display("FAIL: FIFO not empty after drain");
            errors++;
        end

        // Report
        if (errors == 0)
            $display("PASS: tb_fifo_basic — all checks passed");
        else
            $display("FAIL: tb_fifo_basic — %0d error(s)", errors);

        $finish;
    end

endmodule

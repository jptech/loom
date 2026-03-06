// Basic ALU test — verify each operation with known inputs
//
// Self-checking with assertions. Tagged [smoke] in component.toml.
`timescale 1ns / 1ps

module tb_alu_basic;

    parameter WIDTH = 8;

    logic             clk;
    logic             rst_n;
    logic [WIDTH-1:0] a, b;
    logic [2:0]       op;
    logic [WIDTH-1:0] result;
    logic             zero_flag;

    alu #(.WIDTH(WIDTH)) dut (.*);

    // Clock — 10 ns period
    initial clk = 0;
    always #5 clk = ~clk;

    integer errors;

    task automatic apply(
        input logic [WIDTH-1:0] in_a,
        input logic [WIDTH-1:0] in_b,
        input logic [2:0]       in_op,
        input logic [WIDTH-1:0] expected,
        input string            op_name
    );
        @(posedge clk);
        a  <= in_a;
        b  <= in_b;
        op <= in_op;
        @(posedge clk);  // wait for registered output
        @(negedge clk);  // sample at negedge for stability
        if (result !== expected) begin
            $display("FAIL: %s — a=%02X b=%02X expected=%02X got=%02X",
                     op_name, in_a, in_b, expected, result);
            errors++;
        end
    endtask

    initial begin
        errors = 0;
        rst_n  = 1'b0;
        a = '0; b = '0; op = '0;

        repeat (3) @(posedge clk);
        rst_n = 1'b1;
        @(posedge clk);

        // ADD
        apply(8'h10, 8'h20, 3'b000, 8'h30, "ADD");
        apply(8'hFF, 8'h01, 3'b000, 8'h00, "ADD overflow");

        // SUB
        apply(8'h50, 8'h20, 3'b001, 8'h30, "SUB");
        apply(8'h00, 8'h01, 3'b001, 8'hFF, "SUB underflow");

        // AND
        apply(8'hF0, 8'h3C, 3'b010, 8'h30, "AND");

        // OR
        apply(8'hF0, 8'h0F, 3'b011, 8'hFF, "OR");

        // XOR
        apply(8'hAA, 8'h55, 3'b100, 8'hFF, "XOR");
        apply(8'hAA, 8'hAA, 3'b100, 8'h00, "XOR same");

        // NOT
        apply(8'hAA, 8'h00, 3'b101, 8'h55, "NOT");

        // Zero flag check: result of XOR same should be 0
        a  <= 8'hCC;
        b  <= 8'hCC;
        op <= 3'b100; // XOR
        @(posedge clk);
        @(negedge clk);
        if (!zero_flag) begin
            $display("FAIL: zero_flag not set when result is 0");
            errors++;
        end

        // Non-zero flag check
        a  <= 8'h01;
        b  <= 8'h00;
        op <= 3'b000; // ADD
        @(posedge clk);
        @(negedge clk);
        if (zero_flag) begin
            $display("FAIL: zero_flag set when result is non-zero");
            errors++;
        end

        // Report
        if (errors == 0)
            $display("PASS: tb_alu_basic — all checks passed");
        else
            $display("FAIL: tb_alu_basic — %0d error(s)", errors);

        $finish;
    end

endmodule

// Randomized ALU test with concurrent checkers
//
// Uses fork/join to run parallel stimulus and checking, demonstrating
// the [tests.requires] fork_join = true gating mechanism.  Simulators
// without full fork/join support (e.g., Icarus) will skip this test.
`timescale 1ns / 1ps

module tb_alu_random;

    parameter WIDTH = 8;
    parameter NUM_ITERS = 50;

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
    integer seed;
    integer i;

    // Reference model — compute expected result
    function automatic logic [WIDTH-1:0] alu_ref(
        input logic [WIDTH-1:0] a_in,
        input logic [WIDTH-1:0] b_in,
        input logic [2:0]       op_in
    );
        case (op_in)
            3'b000:  return a_in + b_in;
            3'b001:  return a_in - b_in;
            3'b010:  return a_in & b_in;
            3'b011:  return a_in | b_in;
            3'b100:  return a_in ^ b_in;
            3'b101:  return ~a_in;
            default: return '0;
        endcase
    endfunction

    initial begin
        if (!$value$plusargs("seed=%d", seed))
            seed = 12345;

        errors = 0;
        rst_n  = 1'b0;
        a = '0; b = '0; op = '0;

        repeat (3) @(posedge clk);
        rst_n = 1'b1;
        @(posedge clk);

        // Concurrent stimulus + checker using fork/join
        fork
            // Stimulus thread
            begin
                for (i = 0; i < NUM_ITERS; i++) begin
                    @(posedge clk);
                    a  <= $urandom(seed) & {WIDTH{1'b1}};
                    b  <= $urandom(seed) & {WIDTH{1'b1}};
                    op <= $urandom(seed) % 6;
                end
            end

            // Checker thread (1-cycle latency for registered output)
            begin
                logic [WIDTH-1:0] prev_a, prev_b, expected;
                logic [2:0] prev_op;

                @(posedge clk); // skip first cycle (reset settling)

                for (int j = 0; j < NUM_ITERS; j++) begin
                    // Capture inputs
                    prev_a  = a;
                    prev_b  = b;
                    prev_op = op;

                    @(posedge clk); // wait for registered output
                    @(negedge clk); // sample at negedge

                    expected = alu_ref(prev_a, prev_b, prev_op);
                    if (result !== expected) begin
                        $display("FAIL: iter=%0d op=%0d a=%02X b=%02X exp=%02X got=%02X",
                                 j, prev_op, prev_a, prev_b, expected, result);
                        errors++;
                    end

                    // Check zero flag
                    if ((expected == '0) && !zero_flag) begin
                        $display("FAIL: iter=%0d zero_flag not set for zero result", j);
                        errors++;
                    end
                    if ((expected != '0) && zero_flag) begin
                        $display("FAIL: iter=%0d zero_flag set for non-zero result", j);
                        errors++;
                    end
                end
            end
        join

        // Report
        if (errors == 0)
            $display("PASS: tb_alu_random — %0d iterations passed (seed=%0d)", NUM_ITERS, seed);
        else
            $display("FAIL: tb_alu_random — %0d error(s) in %0d iterations (seed=%0d)",
                     errors, NUM_ITERS, seed);

        $finish;
    end

endmodule

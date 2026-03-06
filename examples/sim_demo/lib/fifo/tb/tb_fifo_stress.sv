// FIFO stress test — fill/drain, boundary conditions, random stimulus
//
// Tagged [regression] in component.toml.  Supports +seed=N plusarg.
`timescale 1ns / 1ps

module tb_fifo_stress;

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

    // Clock — 10 ns period
    initial clk = 0;
    always #5 clk = ~clk;

    integer errors;
    integer seed;
    integer i;
    logic [WIDTH-1:0] expected_queue [$];
    logic [WIDTH-1:0] got;

    initial begin
        // Seed support
        if (!$value$plusargs("seed=%d", seed))
            seed = 42;

        errors = 0;
        rst_n  = 1'b0;
        wr_en  = 1'b0;
        rd_en  = 1'b0;
        wr_data = '0;

        repeat (3) @(posedge clk);
        rst_n = 1'b1;
        @(posedge clk);

        // --- Test 1: Fill to full ---
        $display("[stress] Filling FIFO to capacity (%0d entries)", DEPTH);
        for (i = 0; i < DEPTH; i++) begin
            @(posedge clk);
            wr_en   <= 1'b1;
            wr_data <= i[WIDTH-1:0];
            expected_queue.push_back(i[WIDTH-1:0]);
        end
        @(posedge clk);
        wr_en <= 1'b0;

        @(posedge clk);
        if (!full) begin
            $display("FAIL: FIFO not full after %0d writes", DEPTH);
            errors++;
        end

        // Write while full — should be ignored
        @(posedge clk);
        wr_en   <= 1'b1;
        wr_data <= 8'hFF;
        @(posedge clk);
        wr_en <= 1'b0;

        @(posedge clk);
        if (count !== DEPTH[$clog2(DEPTH):0]) begin
            $display("FAIL: Count changed on write-while-full (count=%0d)", count);
            errors++;
        end

        // --- Test 2: Drain completely ---
        $display("[stress] Draining FIFO");
        for (i = 0; i < DEPTH; i++) begin
            @(posedge clk);
            rd_en <= 1'b1;
            @(posedge clk);
            rd_en <= 1'b0;
            got = rd_data;
            if (expected_queue.size() > 0) begin
                logic [WIDTH-1:0] exp;
                exp = expected_queue.pop_front();
                if (got !== exp) begin
                    $display("FAIL: Drain[%0d] expected %02X got %02X", i, exp, got);
                    errors++;
                end
            end
        end

        @(posedge clk);
        if (!empty) begin
            $display("FAIL: FIFO not empty after drain");
            errors++;
        end

        // Read while empty — should be ignored
        @(posedge clk);
        rd_en <= 1'b1;
        @(posedge clk);
        rd_en <= 1'b0;

        @(posedge clk);
        if (count !== 0) begin
            $display("FAIL: Count changed on read-while-empty (count=%0d)", count);
            errors++;
        end

        // --- Test 3: Simultaneous read/write ---
        $display("[stress] Simultaneous read/write at half-full");
        // Fill half
        for (i = 0; i < DEPTH/2; i++) begin
            @(posedge clk);
            wr_en   <= 1'b1;
            wr_data <= (i + 8'h80);
        end
        @(posedge clk);
        wr_en <= 1'b0;

        // Simultaneous R+W for 8 cycles — count should stay constant
        @(posedge clk);
        for (i = 0; i < 8; i++) begin
            @(posedge clk);
            wr_en   <= 1'b1;
            wr_data <= $urandom(seed) & {WIDTH{1'b1}};
            rd_en   <= 1'b1;
        end
        @(posedge clk);
        wr_en <= 1'b0;
        rd_en <= 1'b0;

        @(posedge clk);
        if (count !== DEPTH/2) begin
            $display("FAIL: Count drifted during simultaneous R/W (count=%0d, expected %0d)",
                     count, DEPTH/2);
            errors++;
        end

        // --- Report ---
        if (errors == 0)
            $display("PASS: tb_fifo_stress — all checks passed (seed=%0d)", seed);
        else
            $display("FAIL: tb_fifo_stress — %0d error(s) (seed=%0d)", errors, seed);

        $finish;
    end

endmodule

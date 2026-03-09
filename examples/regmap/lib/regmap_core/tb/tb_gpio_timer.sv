// tb_gpio_timer_top.sv — Basic testbench for GPIO+Timer peripheral
//
// Uses the simple bus wrapper for direct register read/write testing.

`timescale 1ns / 1ps

module tb_gpio_timer_top;

    // Address constants (match generated register file)
    localparam logic [7:0] ADDR_GPIO_DIR      = 8'h00;
    localparam logic [7:0] ADDR_GPIO_OUT      = 8'h04;
    localparam logic [7:0] ADDR_GPIO_IN       = 8'h08;
    localparam logic [7:0] ADDR_IRQ_ENABLE    = 8'h0C;
    localparam logic [7:0] ADDR_IRQ_STATUS    = 8'h10;
    localparam logic [7:0] ADDR_IRQ_CLEAR     = 8'h14;
    localparam logic [7:0] ADDR_TIMER_CTRL    = 8'h18;
    localparam logic [7:0] ADDR_TIMER_COUNT   = 8'h1C;
    localparam logic [7:0] ADDR_TIMER_COMPARE = 8'h20;
    localparam logic [7:0] ADDR_ID            = 8'h24;

    logic        clk = 0;
    logic        rst_n;

    // Simple bus
    logic [7:0]  bus_addr;
    logic        bus_wr;
    logic        bus_rd;
    logic [31:0] bus_wdata;
    logic [31:0] bus_rdata;
    logic        bus_ready;

    // GPIO
    logic [15:0] gpio_i;
    logic [15:0] gpio_o;
    logic [15:0] gpio_oe;
    logic        irq;

    always #5 clk = ~clk;

    simple_regmap #(
        .GPIO_WIDTH(16),
        .TIMER_WIDTH(32),
        .ENABLE_INTERRUPTS(1)
    ) dut (
        .clk      (clk),
        .rst_n    (rst_n),
        .bus_addr (bus_addr),
        .bus_wr   (bus_wr),
        .bus_rd   (bus_rd),
        .bus_wdata(bus_wdata),
        .bus_rdata(bus_rdata),
        .bus_ready(bus_ready),
        .gpio_i   (gpio_i),
        .gpio_o   (gpio_o),
        .gpio_oe  (gpio_oe),
        .irq      (irq)
    );

    // Bus tasks
    task bus_write(input [7:0] addr, input [31:0] data);
        @(posedge clk);
        bus_addr  <= addr;
        bus_wdata <= data;
        bus_wr    <= 1'b1;
        bus_rd    <= 1'b0;
        @(posedge clk);
        bus_wr <= 1'b0;
    endtask

    task bus_read(input [7:0] addr, output [31:0] data);
        @(posedge clk);
        bus_addr <= addr;
        bus_wr   <= 1'b0;
        bus_rd   <= 1'b1;
        @(posedge clk);
        data   = bus_rdata;
        bus_rd <= 1'b0;
    endtask

    logic [31:0] rd_val;

    initial begin
        rst_n    = 0;
        bus_addr = 0;
        bus_wr   = 0;
        bus_rd   = 0;
        bus_wdata = 0;
        gpio_i   = 16'hA5A5;

        repeat (4) @(posedge clk);
        rst_n = 1;

        // Read ID register
        bus_read(ADDR_ID, rd_val);
        assert(rd_val == 32'hCA5B_1A01) else $error("ID mismatch: %h", rd_val);

        // Configure GPIO direction (all output)
        bus_write(ADDR_GPIO_DIR, 32'h0000_FFFF);

        // Write GPIO output
        bus_write(ADDR_GPIO_OUT, 32'h0000_1234);
        assert(gpio_o == 16'h1234) else $error("GPIO output mismatch");

        // Read GPIO input
        bus_read(ADDR_GPIO_IN, rd_val);
        assert(rd_val[15:0] == 16'hA5A5) else $error("GPIO input mismatch: %h", rd_val);

        // Start timer with auto-reload, compare = 10
        bus_write(ADDR_TIMER_COMPARE, 32'd10);
        bus_write(ADDR_IRQ_ENABLE, 32'h0000_0001);
        bus_write(ADDR_TIMER_CTRL, 32'h0000_0003);

        // Wait for timer match
        repeat (20) @(posedge clk);

        // Check IRQ
        bus_read(ADDR_IRQ_STATUS, rd_val);
        if (rd_val[0]) begin
            $display("Timer IRQ asserted as expected");
            bus_write(ADDR_IRQ_CLEAR, 32'h0000_0001);
        end

        $display("PASS: all basic tests passed");
        $finish;
    end

endmodule

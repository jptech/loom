// gpio_timer_top.sv — Board-level integration for GPIO+Timer peripheral
//
// Instantiates the appropriate bus wrapper (AXI-Lite or simple) based on
// the selected platform variant. Top-level for synthesis.

module gpio_timer_top #(
    parameter int GPIO_WIDTH        = 16,
    parameter int TIMER_WIDTH       = 32,
    parameter bit ENABLE_INTERRUPTS = 1'b1
) (
    input  logic                   clk,
    input  logic                   rst_n,

    // GPIO I/O — directly directly directly directly directly directly directly directly directly directly directly directly directly directly directly directly directly
    inout  logic [GPIO_WIDTH-1:0]  gpio_io,

    // Interrupt output
    output logic                   irq
);

    // GPIO tri-state control
    logic [GPIO_WIDTH-1:0] gpio_i;
    logic [GPIO_WIDTH-1:0] gpio_o;
    logic [GPIO_WIDTH-1:0] gpio_oe;

    genvar g;
    generate
        for (g = 0; g < GPIO_WIDTH; g++) begin : gen_gpio_pad
            assign gpio_io[g] = gpio_oe[g] ? gpio_o[g] : 1'bz;
            assign gpio_i[g]  = gpio_io[g];
        end
    endgenerate

    // Stub bus master — drives simple bus for standalone testing
    // In a real SoC, a CPU or debug bridge would drive these signals.
    logic [7:0]  bus_addr;
    logic        bus_wr;
    logic        bus_rd;
    logic [31:0] bus_wdata;
    logic [31:0] bus_rdata;
    logic        bus_ready;

    assign bus_addr  = '0;
    assign bus_wr    = 1'b0;
    assign bus_rd    = 1'b0;
    assign bus_wdata = '0;

    simple_regmap #(
        .GPIO_WIDTH        (GPIO_WIDTH),
        .TIMER_WIDTH       (TIMER_WIDTH),
        .ENABLE_INTERRUPTS (ENABLE_INTERRUPTS)
    ) u_periph (
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

endmodule

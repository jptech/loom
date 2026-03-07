// simple_regmap.sv — Simple memory-mapped bus wrapper for GPIO+Timer peripheral
//
// Single-cycle bus access: directly wires bus signals to the gpio_timer_core
// native bus interface. Suitable for small FPGAs without AXI infrastructure.

module simple_regmap #(
    parameter int GPIO_WIDTH = 16,
    parameter int TIMER_WIDTH = 32,
    parameter bit ENABLE_INTERRUPTS = 1'b1
) (
    input  logic        clk,
    input  logic        rst_n,

    // Simple bus interface
    input  logic [7:0]  bus_addr,
    input  logic        bus_wr,
    input  logic        bus_rd,
    input  logic [31:0] bus_wdata,
    output logic [31:0] bus_rdata,
    output logic        bus_ready,

    // GPIO pins
    input  logic [GPIO_WIDTH-1:0]  gpio_i,
    output logic [GPIO_WIDTH-1:0]  gpio_o,
    output logic [GPIO_WIDTH-1:0]  gpio_oe,

    // Interrupt
    output logic                   irq
);

    // Direct passthrough to core
    gpio_timer_core #(
        .GPIO_WIDTH        (GPIO_WIDTH),
        .TIMER_WIDTH       (TIMER_WIDTH),
        .ENABLE_INTERRUPTS (ENABLE_INTERRUPTS)
    ) u_core (
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

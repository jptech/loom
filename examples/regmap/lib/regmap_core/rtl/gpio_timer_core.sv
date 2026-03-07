// gpio_timer_core.sv — GPIO + Timer + IRQ logic
//
// Combines the generated register file with GPIO I/O control,
// a configurable timer with auto-reload, and interrupt generation.

module gpio_timer_core #(
    parameter int GPIO_WIDTH  = 16,
    parameter int TIMER_WIDTH = 32,
    parameter bit ENABLE_INTERRUPTS = 1'b1,
    parameter int ADDR_WIDTH  = 8,
    parameter int DATA_WIDTH  = 32
) (
    input  logic                   clk,
    input  logic                   rst_n,

    // Native bus interface to register file
    input  logic [ADDR_WIDTH-1:0]  bus_addr,
    input  logic                   bus_wr,
    input  logic                   bus_rd,
    input  logic [DATA_WIDTH-1:0]  bus_wdata,
    output logic [DATA_WIDTH-1:0]  bus_rdata,
    output logic                   bus_ready,

    // GPIO pins
    input  logic [GPIO_WIDTH-1:0]  gpio_i,
    output logic [GPIO_WIDTH-1:0]  gpio_o,
    output logic [GPIO_WIDTH-1:0]  gpio_oe,

    // Interrupt
    output logic                   irq
);

    // Register file hardware interface signals
    logic [DATA_WIDTH-1:0] hw_gpio_dir;
    logic [DATA_WIDTH-1:0] hw_gpio_out;
    logic [DATA_WIDTH-1:0] hw_gpio_in;
    logic [DATA_WIDTH-1:0] hw_irq_enable;
    logic [DATA_WIDTH-1:0] hw_irq_status;
    logic [DATA_WIDTH-1:0] hw_irq_clear;
    logic [DATA_WIDTH-1:0] hw_timer_ctrl;
    logic [DATA_WIDTH-1:0] hw_timer_count;
    logic [DATA_WIDTH-1:0] hw_timer_compare;
    logic [DATA_WIDTH-1:0] hw_id;

    // Instantiate generated register file
    gpio_timer_regs u_regs (
        .clk       (clk),
        .rst_n     (rst_n),
        .bus_addr  (bus_addr),
        .bus_wr    (bus_wr),
        .bus_rd    (bus_rd),
        .bus_wdata (bus_wdata),
        .bus_rdata (bus_rdata),
        .bus_ready (bus_ready),

        .hw_gpio_dir     (hw_gpio_dir),
        .hw_gpio_out     (hw_gpio_out),
        .hw_gpio_in      (hw_gpio_in),
        .hw_irq_enable   (hw_irq_enable),
        .hw_irq_status   (hw_irq_status),
        .hw_irq_clear    (hw_irq_clear),
        .hw_timer_ctrl   (hw_timer_ctrl),
        .hw_timer_count  (hw_timer_count),
        .hw_timer_compare(hw_timer_compare),
        .hw_id           (hw_id)
    );

    // --- GPIO ---
    assign gpio_o  = hw_gpio_out[GPIO_WIDTH-1:0];
    assign gpio_oe = hw_gpio_dir[GPIO_WIDTH-1:0];
    assign hw_gpio_in = {{(DATA_WIDTH-GPIO_WIDTH){1'b0}}, gpio_i};

    // --- Timer ---
    logic timer_en;
    logic timer_auto_reload;
    logic [TIMER_WIDTH-1:0] timer_cnt;
    logic timer_match;

    assign timer_en          = hw_timer_ctrl[0];
    assign timer_auto_reload = hw_timer_ctrl[1];
    assign hw_timer_count    = {{(DATA_WIDTH-TIMER_WIDTH){1'b0}}, timer_cnt};

    assign timer_match = (timer_cnt == hw_timer_compare[TIMER_WIDTH-1:0]);

    always_ff @(posedge clk or negedge rst_n) begin
        if (!rst_n) begin
            timer_cnt <= '0;
        end else if (timer_en) begin
            if (timer_match) begin
                timer_cnt <= timer_auto_reload ? '0 : timer_cnt;
            end else begin
                timer_cnt <= timer_cnt + 1;
            end
        end
    end

    // --- IRQ ---
    logic [DATA_WIDTH-1:0] irq_raw;
    assign irq_raw = {{(DATA_WIDTH-1){1'b0}}, timer_match};

    assign hw_irq_status = irq_raw;

    // Peripheral ID
    assign hw_id = 32'hCA5B_1A01;

    generate
        if (ENABLE_INTERRUPTS) begin : gen_irq
            assign irq = |(hw_irq_status & hw_irq_enable);
        end else begin : gen_no_irq
            assign irq = 1'b0;
        end
    endgenerate

endmodule

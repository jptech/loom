// axilite_regmap.sv — AXI-Lite protocol wrapper for GPIO+Timer peripheral
//
// Translates AXI-Lite slave transactions into native bus reads/writes
// for the gpio_timer_core.

module axilite_regmap #(
    parameter int GPIO_WIDTH = 16,
    parameter int TIMER_WIDTH = 32,
    parameter bit ENABLE_INTERRUPTS = 1'b1
) (
    input  logic        clk,
    input  logic        rst_n,

    // AXI-Lite slave interface
    input  logic [7:0]  s_axil_awaddr,
    input  logic        s_axil_awvalid,
    output logic        s_axil_awready,
    input  logic [31:0] s_axil_wdata,
    input  logic [3:0]  s_axil_wstrb,
    input  logic        s_axil_wvalid,
    output logic        s_axil_wready,
    output logic [1:0]  s_axil_bresp,
    output logic        s_axil_bvalid,
    input  logic        s_axil_bready,
    input  logic [7:0]  s_axil_araddr,
    input  logic        s_axil_arvalid,
    output logic        s_axil_arready,
    output logic [31:0] s_axil_rdata,
    output logic [1:0]  s_axil_rresp,
    output logic        s_axil_rvalid,
    input  logic        s_axil_rready,

    // GPIO pins
    input  logic [GPIO_WIDTH-1:0]  gpio_i,
    output logic [GPIO_WIDTH-1:0]  gpio_o,
    output logic [GPIO_WIDTH-1:0]  gpio_oe,

    // Interrupt
    output logic                   irq
);

    // Internal native bus signals
    logic [7:0]  bus_addr;
    logic        bus_wr;
    logic        bus_rd;
    logic [31:0] bus_wdata;
    logic [31:0] bus_rdata;
    logic        bus_ready;

    // AXI-Lite write channel
    typedef enum logic [1:0] {
        WR_IDLE,
        WR_DATA,
        WR_RESP
    } wr_state_t;

    wr_state_t wr_state;
    logic [7:0] wr_addr;

    always_ff @(posedge clk or negedge rst_n) begin
        if (!rst_n) begin
            wr_state <= WR_IDLE;
            wr_addr  <= '0;
        end else begin
            case (wr_state)
                WR_IDLE: begin
                    if (s_axil_awvalid && s_axil_awready) begin
                        wr_addr  <= s_axil_awaddr;
                        wr_state <= WR_DATA;
                    end
                end
                WR_DATA: begin
                    if (s_axil_wvalid && s_axil_wready) begin
                        wr_state <= WR_RESP;
                    end
                end
                WR_RESP: begin
                    if (s_axil_bvalid && s_axil_bready) begin
                        wr_state <= WR_IDLE;
                    end
                end
                default: wr_state <= WR_IDLE;
            endcase
        end
    end

    assign s_axil_awready = (wr_state == WR_IDLE);
    assign s_axil_wready  = (wr_state == WR_DATA);
    assign s_axil_bvalid  = (wr_state == WR_RESP);
    assign s_axil_bresp   = 2'b00; // OKAY

    // AXI-Lite read channel
    typedef enum logic [1:0] {
        RD_IDLE,
        RD_READ,
        RD_RESP
    } rd_state_t;

    rd_state_t rd_state;
    logic [7:0]  rd_addr;
    logic [31:0] rd_data_reg;

    always_ff @(posedge clk or negedge rst_n) begin
        if (!rst_n) begin
            rd_state    <= RD_IDLE;
            rd_addr     <= '0;
            rd_data_reg <= '0;
        end else begin
            case (rd_state)
                RD_IDLE: begin
                    if (s_axil_arvalid && s_axil_arready) begin
                        rd_addr  <= s_axil_araddr;
                        rd_state <= RD_READ;
                    end
                end
                RD_READ: begin
                    rd_data_reg <= bus_rdata;
                    rd_state    <= RD_RESP;
                end
                RD_RESP: begin
                    if (s_axil_rvalid && s_axil_rready) begin
                        rd_state <= RD_IDLE;
                    end
                end
                default: rd_state <= RD_IDLE;
            endcase
        end
    end

    assign s_axil_arready = (rd_state == RD_IDLE);
    assign s_axil_rvalid  = (rd_state == RD_RESP);
    assign s_axil_rdata   = rd_data_reg;
    assign s_axil_rresp   = 2'b00; // OKAY

    // Mux native bus
    assign bus_addr  = bus_wr ? wr_addr : rd_addr;
    assign bus_wr    = (wr_state == WR_DATA) && s_axil_wvalid;
    assign bus_wdata = s_axil_wdata;
    assign bus_rd    = (rd_state == RD_READ);

    // Core instantiation
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

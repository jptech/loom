// UART receiver — 8N1, 16x oversampling, configurable baud rate
module uart_rx #(
    parameter int CLK_FREQ = 50_000_000,
    parameter int BAUD     = 115200
) (
    input  logic       clk,
    input  logic       rst_n,
    input  logic       rx,
    output logic [7:0] rx_data,
    output logic       rx_valid
);

    localparam int CLKS_PER_BIT = CLK_FREQ / BAUD;
    localparam int CNT_WIDTH    = $clog2(CLKS_PER_BIT + 1);
    localparam int HALF_BIT     = CLKS_PER_BIT / 2;

    typedef enum logic [1:0] {
        IDLE  = 2'b00,
        START = 2'b01,
        DATA  = 2'b10,
        STOP  = 2'b11
    } state_t;

    state_t                state;
    logic [CNT_WIDTH-1:0]  baud_cnt;
    logic [2:0]            bit_idx;
    logic [7:0]            shift_reg;

    // Input synchronizer (metastability hardening)
    logic rx_sync_0, rx_sync;
    always_ff @(posedge clk or negedge rst_n) begin
        if (!rst_n) begin
            rx_sync_0 <= 1'b1;
            rx_sync   <= 1'b1;
        end else begin
            rx_sync_0 <= rx;
            rx_sync   <= rx_sync_0;
        end
    end

    wire logic baud_tick = (baud_cnt == CNT_WIDTH'(CLKS_PER_BIT - 1));
    wire logic mid_bit   = (baud_cnt == CNT_WIDTH'(HALF_BIT));

    always_ff @(posedge clk or negedge rst_n) begin
        if (!rst_n) begin
            state     <= IDLE;
            baud_cnt  <= '0;
            bit_idx   <= '0;
            shift_reg <= '0;
            rx_data   <= '0;
            rx_valid  <= 1'b0;
        end else begin
            rx_valid <= 1'b0;  // default: one-cycle pulse

            case (state)
                IDLE: begin
                    baud_cnt <= '0;
                    bit_idx  <= '0;
                    if (!rx_sync) begin  // falling edge = start bit
                        state <= START;
                    end
                end

                START: begin
                    if (mid_bit) begin
                        if (!rx_sync) begin
                            // Confirmed start bit at midpoint
                            baud_cnt <= '0;
                            state    <= DATA;
                        end else begin
                            // False start — glitch
                            state <= IDLE;
                        end
                    end else begin
                        baud_cnt <= baud_cnt + 1'b1;
                    end
                end

                DATA: begin
                    if (baud_tick) begin
                        baud_cnt <= '0;
                        shift_reg[bit_idx] <= rx_sync;
                        if (bit_idx == 3'd7) begin
                            state <= STOP;
                        end else begin
                            bit_idx <= bit_idx + 1'b1;
                        end
                    end else begin
                        baud_cnt <= baud_cnt + 1'b1;
                    end
                end

                STOP: begin
                    if (baud_tick) begin
                        baud_cnt <= '0;
                        if (rx_sync) begin
                            // Valid stop bit
                            rx_data  <= shift_reg;
                            rx_valid <= 1'b1;
                        end
                        state <= IDLE;
                    end else begin
                        baud_cnt <= baud_cnt + 1'b1;
                    end
                end
            endcase
        end
    end

endmodule

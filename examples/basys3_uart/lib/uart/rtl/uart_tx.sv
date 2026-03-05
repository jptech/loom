// UART transmitter — 8N1, configurable baud rate
module uart_tx #(
    parameter int CLK_FREQ = 50_000_000,
    parameter int BAUD     = 115200
) (
    input  logic       clk,
    input  logic       rst_n,
    input  logic [7:0] tx_data,
    input  logic       tx_valid,
    output logic       tx,
    output logic       tx_ready
);

    localparam int CLKS_PER_BIT = CLK_FREQ / BAUD;
    localparam int CNT_WIDTH    = $clog2(CLKS_PER_BIT + 1);

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

    wire logic baud_tick = (baud_cnt == CNT_WIDTH'(CLKS_PER_BIT - 1));

    always_ff @(posedge clk or negedge rst_n) begin
        if (!rst_n) begin
            state     <= IDLE;
            baud_cnt  <= '0;
            bit_idx   <= '0;
            shift_reg <= '0;
            tx        <= 1'b1;
            tx_ready  <= 1'b1;
        end else begin
            case (state)
                IDLE: begin
                    tx       <= 1'b1;
                    tx_ready <= 1'b1;
                    baud_cnt <= '0;
                    bit_idx  <= '0;
                    if (tx_valid) begin
                        shift_reg <= tx_data;
                        tx_ready  <= 1'b0;
                        state     <= START;
                    end
                end

                START: begin
                    tx <= 1'b0;  // start bit
                    if (baud_tick) begin
                        baud_cnt <= '0;
                        state    <= DATA;
                    end else begin
                        baud_cnt <= baud_cnt + 1'b1;
                    end
                end

                DATA: begin
                    tx <= shift_reg[bit_idx];
                    if (baud_tick) begin
                        baud_cnt <= '0;
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
                    tx <= 1'b1;  // stop bit
                    if (baud_tick) begin
                        baud_cnt <= '0;
                        state    <= IDLE;
                    end else begin
                        baud_cnt <= baud_cnt + 1'b1;
                    end
                end
            endcase
        end
    end

endmodule

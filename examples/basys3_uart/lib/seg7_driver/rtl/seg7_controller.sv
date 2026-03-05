// 4-digit multiplexed 7-segment display controller for Basys 3
// Uses counter module for ~1 kHz refresh rate
module seg7_controller (
    input  logic       clk,
    input  logic       rst_n,
    input  logic [3:0] digit3,   // leftmost digit (MSD)
    input  logic [3:0] digit2,
    input  logic [3:0] digit1,
    input  logic [3:0] digit0,   // rightmost digit (LSD)
    output logic [6:0] seg,      // cathodes (active-low)
    output logic [3:0] an        // anodes (active-low)
);

    // Refresh counter — use top 2 bits to select active digit
    // At 100 MHz, 17-bit counter overflows at ~763 Hz per digit
    localparam int REFRESH_WIDTH = 17;

    logic [REFRESH_WIDTH-1:0] refresh_count;

    counter #(
        .WIDTH(REFRESH_WIDTH)
    ) u_refresh (
        .clk   (clk),
        .rst_n (rst_n),
        .en    (1'b1),
        .count (refresh_count)
    );

    // Digit select mux
    logic [3:0] current_hex;
    logic [1:0] digit_sel;
    assign digit_sel = refresh_count[REFRESH_WIDTH-1 -: 2];

    always_comb begin
        case (digit_sel)
            2'b00: begin
                current_hex = digit0;
                an          = 4'b1110;
            end
            2'b01: begin
                current_hex = digit1;
                an          = 4'b1101;
            end
            2'b10: begin
                current_hex = digit2;
                an          = 4'b1011;
            end
            2'b11: begin
                current_hex = digit3;
                an          = 4'b0111;
            end
        endcase
    end

    // Hex-to-7-segment decoder
    hex_to_seg7 u_decoder (
        .hex (current_hex),
        .seg (seg)
    );

endmodule

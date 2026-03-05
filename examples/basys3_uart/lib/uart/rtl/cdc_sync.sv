// Generic 2-FF synchronizer for clock domain crossing
module cdc_sync #(
    parameter int WIDTH = 1
) (
    input  logic             clk,
    input  logic             rst_n,
    input  logic [WIDTH-1:0] d,
    output logic [WIDTH-1:0] q
);

    logic [WIDTH-1:0] sync_0;
    logic [WIDTH-1:0] sync_1;

    always_ff @(posedge clk or negedge rst_n) begin
        if (!rst_n) begin
            sync_0 <= '0;
            sync_1 <= '0;
        end else begin
            sync_0 <= d;
            sync_1 <= sync_0;
        end
    end

    assign q = sync_1;

endmodule

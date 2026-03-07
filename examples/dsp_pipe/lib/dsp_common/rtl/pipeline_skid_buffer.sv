// pipeline_skid_buffer.sv — AXI-Stream skid buffer for backpressure decoupling
//
// Two-register implementation: allows downstream to stall for one cycle
// without blocking the upstream producer. Parameterized data width.

module pipeline_skid_buffer #(
    parameter int WIDTH = 16
) (
    input  logic             clk,
    input  logic             rst_n,

    // Upstream (input) AXI-Stream
    input  logic [WIDTH-1:0] s_tdata,
    input  logic             s_tvalid,
    output logic             s_tready,

    // Downstream (output) AXI-Stream
    output logic [WIDTH-1:0] m_tdata,
    output logic             m_tvalid,
    input  logic             m_tready
);

    logic [WIDTH-1:0] buf_data;
    logic             buf_valid;

    // Accept upstream data when buffer is empty or downstream is consuming
    assign s_tready = !buf_valid || m_tready;

    // Output mux: prefer buffer if valid, else pass through
    assign m_tdata  = buf_valid ? buf_data : s_tdata;
    assign m_tvalid = buf_valid || s_tvalid;

    always_ff @(posedge clk or negedge rst_n) begin
        if (!rst_n) begin
            buf_data  <= '0;
            buf_valid <= 1'b0;
        end else begin
            if (s_tvalid && s_tready && m_tvalid && !m_tready) begin
                // Downstream stalled, capture incoming data
                buf_data  <= s_tdata;
                buf_valid <= 1'b1;
            end else if (buf_valid && m_tready) begin
                // Buffer consumed
                buf_valid <= 1'b0;
            end
        end
    end

endmodule

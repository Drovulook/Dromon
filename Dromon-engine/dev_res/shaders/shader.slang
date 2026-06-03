static const float2 positions[3] = {
  float2(0.0, -0.5),
  float2(0.5, 0.5),
  float2(-0.5, 0.5),
};

static const float3 colors[3] = {
  float3(1.0, 0.0, 0.0),
  float3(0.0, 1.0, 0.0),
  float3(0.0, 0.0, 1.0),
};

struct VSOutput {
  float4 pos : SV_Position;
  float3 color : COLOR;
};

[shader("vertex")]
VSOutput vertMain(uint vertexID: SV_VertexID) {
  VSOutput output;
  output.pos = float4(positions[vertexID], 0.0, 1.0);
  output.color = colors[vertexID];
  return output;
}

[shader("fragment")]
float4 fragMain(VSOutput vertIn) : SV_Target {
  return float4(vertIn.color, 1.0);
}

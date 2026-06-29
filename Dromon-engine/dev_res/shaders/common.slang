// Déclarations partagées entre `object.slang` et `terrain.slang`.

// set 0, binding 0 : données « caméra », mises à jour une fois par frame.
struct UniformBuffer {
    float4x4 view;
    float4x4 proj;
    float4x4 lightViewProj; // view*proj de la lumière (projection orthographique)
    float4 lightDirection;  // xyz = direction de propagation des rayons
    float4 lightColor;      // rgb = couleur, w = intensité
};
[[vk::binding(0, 0)]]
ConstantBuffer<UniformBuffer> ubo;

// Push constant : matrice « model » + matrice normale (inverse-transposée de la
// 3x3 du model, calculée côté CPU), poussées par objet / par chunk de terrain.
struct PushConstants {
    float4x4 model;
    float4x4 normalMatrix;
};
[[vk::push_constant]]
ConstantBuffer<PushConstants> pc;

// set 2, binding 0 : la shadow map. Sampler2DShadow = sampler de comparaison :
// SampleCmp(uv, ref) renvoie ~1.0 si ref <= profondeur stockée (fragment éclairé),
// ~0.0 sinon (fragment dans l'ombre), avec PCF 2×2 si filtrage linéaire.
[[vk::binding(0, 2)]]
Sampler2DShadow shadowMap;

// Taille d'un texel de la shadow map en coordonnées uv (= 1 / résolution).
// ⚠ doit rester synchronisé avec SHADOW_MAP_RESOLUTION (shadow_map.rs).
static const float SHADOW_MAP_TEXEL_SIZE = 1.0 / 2048.0;

// Renvoie un facteur d'éclairement ∈ [0,1] : 1 = pleinement éclairé, 0 = ombré.
float computeShadow(float4 shadowCoord, float NdotL) {

  // Division perspective (triviale en orthographique, où w = 1).
  float3 proj = shadowCoord.xyz / shadowCoord.w;

  // En dehors du frustum de la lumière (au-delà du far) → considéré éclairé.
  if (proj.z > 1.0) {
    return 1.0;
  }

  // NDC xy ∈ [-1,1] → coordonnées de texture uv ∈ [0,1].
  float2 uv = proj.xy * 0.5 + 0.5;

  // Biais dépendant de la pente : plus la surface est rasante (NdotL faible),
  // plus on décale, pour limiter le « shadow acne ». ⚠ Exprimé en profondeur
  // NORMALISÉE [0,1] : avec `far` grand (terrain, ~600), un même biais NDC vaut
  // beaucoup plus en unités monde → ces valeurs suffisent à tuer l'acné du
  // terrain sans trop décoller l'ombre des petits objets (far ~60).
  float bias = max(0.002, 0.008 * (1.0 - NdotL));

  // PCF 3×3 : on moyenne 9 texels voisins. Chaque tap est déjà filtré 2×2 par le
  // hardware (sampler LINEAR) → bord lissé sans escalier, mais sans le flou
  // excessif d'un noyau trop large.
  float sum = 0.0;
  for (int x = -2; x <= 2; x++) {
    for (int y = -2; y <= 2; y++) {
      float2 offset = float2(x, y) * SHADOW_MAP_TEXEL_SIZE;
      sum += shadowMap.SampleCmp(uv + offset, proj.z - bias);
    }
  }
  return sum / 25; // moyenne des 9 échantillons
}

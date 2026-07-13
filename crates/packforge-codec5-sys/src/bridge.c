#include <stddef.h>

#include "Bcj2.h"

int packforge_bcj2_encode(const unsigned char *input, size_t input_length,
                          unsigned char **outputs, const size_t *capacities,
                          size_t *lengths) {
  CBcj2Enc encoder;
  Bcj2Enc_Init(&encoder);
  Bcj2Enc_SET_FileSize(&encoder, input_length);
  encoder.src = input;
  encoder.srcLim = input + input_length;
  encoder.finishMode = BCJ2_ENC_FINISH_MODE_END_STREAM;
  for (unsigned stream = 0; stream < BCJ2_NUM_STREAMS; ++stream) {
    encoder.bufs[stream] = outputs[stream];
    encoder.lims[stream] = outputs[stream] + capacities[stream];
  }
  Bcj2Enc_Encode(&encoder);
  if (!Bcj2Enc_IsFinished(&encoder) ||
      encoder.state != BCJ2_ENC_STATE_FINISHED || encoder.src != encoder.srcLim)
    return 1;
  size_t reconstructed = 0;
  for (unsigned stream = 0; stream < BCJ2_NUM_STREAMS; ++stream) {
    lengths[stream] = (size_t)(encoder.bufs[stream] - outputs[stream]);
    if (stream != BCJ2_STREAM_RC) reconstructed += lengths[stream];
  }
  return reconstructed == input_length ? 0 : 1;
}

int packforge_bcj2_decode(const unsigned char **inputs, const size_t *lengths,
                          unsigned char *output, size_t output_length) {
  CBcj2Dec decoder;
  Bcj2Dec_Init(&decoder);
  for (unsigned stream = 0; stream < BCJ2_NUM_STREAMS; ++stream) {
    decoder.bufs[stream] = inputs[stream];
    decoder.lims[stream] = inputs[stream] + lengths[stream];
  }
  decoder.dest = output;
  decoder.destLim = output + output_length;
  if (Bcj2Dec_Decode(&decoder) != SZ_OK || decoder.dest != decoder.destLim ||
      !Bcj2Dec_IsMaybeFinished(&decoder))
    return 1;
  for (unsigned stream = 0; stream < BCJ2_NUM_STREAMS; ++stream)
    if (decoder.bufs[stream] != decoder.lims[stream]) return 1;
  return 0;
}


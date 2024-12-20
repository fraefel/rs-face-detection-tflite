use std::path::PathBuf;
use anyhow::Error;
use nalgebra::ComplexField;
use ndarray::{Array2, Array3, Axis};
use opencv::core::{Mat, MatTraitConst, Vec3b, Vector};
use opencv::imgcodecs::imwrite;
use opencv::imgproc::resize;
use tflite::{FlatBufferModel, InterpreterBuilder};
use tflite::ops::builtin::BuiltinOpResolver;
use crate::face_detection_lite::face_landmark::{face_detection_to_roi, FaceLandmark};
use crate::face_detection_lite::transform::{image_to_tensor};
use crate::face_detection_lite::types::{BBox, ImageTensor, Rect};
use crate::face_detection_lite::utils::l2_norm;

enum FeatureCount {
    Feature128,
    Feature512,
}

const IMG_SIZE: i32 = 112;

pub struct FaceEmbeddings {
    model_path: PathBuf,
    model: FlatBufferModel,
}


impl FaceEmbeddings {
    /// `FaceEmbeddings` extracts facial features as an array of 128 or 512 f32 elements
    pub fn new(model_path: Option<String>) -> Result<FaceEmbeddings, Error> {
        let mut model_path_buf: PathBuf;

        if let Some(path) = model_path {
            model_path_buf = PathBuf::from(path);
        } else {
            model_path_buf = PathBuf::from("./models/face_embeddings.tflite");
        }
        let model = FlatBufferModel::build_from_file(model_path_buf.clone())?;

        Ok(FaceEmbeddings {
            model_path: model_path_buf,
            model,
        })
    }

    pub fn infer(&self, image: &Mat, bbox: BBox) -> Result<Array2<f32>, Error> {
        // Init model interpreter
        let resolver = BuiltinOpResolver::default();
        let builder = InterpreterBuilder::new(&self.model, &resolver)?;
        let mut interpreter = builder.build()?;
        interpreter.allocate_tensors()?;


        let roi_image = crop_image_to_bbox(&image, bbox)?;
        let image_data = image_to_tensor(&roi_image, None, Some((IMG_SIZE, IMG_SIZE)), false, (0.0, 1.0), false)?;

        // Add additional axis
        let input_data = image_data
            .tensor_data
            .into_dimensionality::<ndarray::IxDyn>()
            .unwrap()
            .insert_axis(Axis(0));

        // Infer model with input data
        let inputs = interpreter.inputs().to_vec();
        let input_index = inputs[0];
        let sub_tensor: Vec<f32> = input_data.into_iter().collect();
        interpreter
            .tensor_data_mut(input_index)?
            .copy_from_slice(&sub_tensor);
        interpreter.invoke()?;

        // retrieve outputs
        let outputs = interpreter.outputs().to_vec();
        let embeddings_index = outputs[0];

        // retrieve output info
        let bbox_info = interpreter
            .tensor_info(embeddings_index)
            .ok_or(Error::msg("missing embeddings outputs info"))?;

        let raw_embeddings: &[f32] = interpreter.tensor_data(embeddings_index).unwrap();
        let embeddings: Array2<f32> =
            Array2::from_shape_vec((bbox_info.dims[0], bbox_info.dims[1]), raw_embeddings.to_vec())?;

        let norm_embeddings = l2_norm(&embeddings);

        Ok(norm_embeddings)
    }
}


/// `crop_image_to_bbox` crops image to the bounding box region.
/// * Args:
///
///     - image (`&Mat`): Input OpenCV matrix.
///     - rect (`BBox`): Bounding box of the ROI.
///
/// * Returns:
///    `Mat` - Cropped OpenCV matrix.
fn crop_image_to_bbox(image: &Mat, rect: BBox) -> Result<Mat, Error> {
    let cropped_image = Mat::roi(image, opencv::core::Rect {
        x: rect.xmin as i32,
        y: rect.ymin as i32,
        width: (rect.xmax - rect.xmin) as i32,
        height: (rect.ymax - rect.ymin) as i32,
    }).unwrap();
    Ok(cropped_image.clone_pointee())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::face_detection_lite::face_detection::{FaceDetection, FaceDetectionModel};
    use crate::face_detection_lite::utils::{convert_image_to_mat, similarity_score};
    use opencv::core::MatTraitConst;

    #[test]
    fn test_face_embeddings() {
        let face_detection = FaceDetection::new(FaceDetectionModel::BackCamera, None).unwrap();

        let im_bytes1: &[u8] = include_bytes!("../../test_data/russ_cox_1.jpg");
        let image1 = convert_image_to_mat(im_bytes1).unwrap();
        let img_shape1 = image1.size().unwrap();
        let faces1 = face_detection.infer(&image1, None).unwrap();
        let bbox1 = faces1[0].bbox();
        let face_embeddings1 = FaceEmbeddings::new(None).unwrap();
        let embeddings1 = face_embeddings1.infer(&image1, bbox1.scale((img_shape1.width as f64, img_shape1.height as f64))).unwrap();
        println!("embeddings: {:?}", embeddings1);

        let im_bytes2: &[u8] = include_bytes!("../../test_data/russ_cox_2.jpg");
        let image2 = convert_image_to_mat(im_bytes2).unwrap();
        let img_shape2 = image2.size().unwrap();
        let faces2 = face_detection.infer(&image2, None).unwrap();
        let bbox2 = faces2[0].bbox();
        let face_embeddings2 = FaceEmbeddings::new(None).unwrap();
        let embeddings2 = face_embeddings2.infer(&image2, bbox2.scale((img_shape2.width as f64, img_shape2.height as f64))).unwrap();
        println!("embeddings: {:?}", embeddings2);

        let (v1, _) = embeddings1.into_raw_vec_and_offset();
        let (v2, _) = embeddings2.into_raw_vec_and_offset();

        let similarity_score = similarity_score(&v1, &v2);
        println!("similarity_score: {:?}", similarity_score);

    }
}
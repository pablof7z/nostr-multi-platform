use std::any::Any;

pub(crate) trait SourceValue: Send + Sync {
    fn as_any(&self) -> &dyn Any;
    fn equals(&self, other: &dyn SourceValue) -> bool;
    fn type_name(&self) -> &'static str;
}

impl<T> SourceValue for T
where
    T: Clone + Eq + Send + Sync + 'static,
{
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn equals(&self, other: &dyn SourceValue) -> bool {
        other.as_any().downcast_ref::<T>() == Some(self)
    }

    fn type_name(&self) -> &'static str {
        std::any::type_name::<T>()
    }
}

pub(crate) fn boxed_value<T>(value: T) -> Box<dyn SourceValue>
where
    T: Clone + Eq + Send + Sync + 'static,
{
    Box::new(value)
}

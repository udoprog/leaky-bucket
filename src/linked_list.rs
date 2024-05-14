//! An intrusive linked list of waiters.

use core::cell::UnsafeCell;
use core::fmt;
use core::marker;
use core::ops;
use core::ptr;

#[repr(C)]
struct Pointers<T> {
    /// The next node.
    next: Option<ptr::NonNull<Node<T>>>,
    /// The previous node.
    prev: Option<ptr::NonNull<Node<T>>>,
    /// Avoids noalias heuristics from kicking in on references to a
    /// `Pointers<T>` struct.
    _pin: marker::PhantomPinned,
}

pub struct Node<T> {
    /// We only access pointers through raw pointer manipulation to avoid
    /// having noalias attributes being generated in the future.
    ///
    /// If we don't do this, intermediate references being used will cause
    /// noalias to be added, which is a broken assumption.
    pointers: UnsafeCell<Pointers<T>>,
    /// The value inside of the node.
    value: T,
}

// Safety: Node doesn't do anything inherently unsafe, it all depends on what's
// stored in it.
unsafe impl<T> Send for Node<T> where T: Send {}
unsafe impl<T> Sync for Node<T> where T: Sync {}

impl<T> Node<T> {
    /// Construct a new unlinked node.
    pub(crate) const fn new(value: T) -> Self {
        Self {
            pointers: UnsafeCell::new(Pointers {
                next: None,
                prev: None,
                _pin: marker::PhantomPinned,
            }),
            value,
        }
    }

    /// Access the value in the node.
    #[inline]
    pub(crate) fn value(&self) -> &T {
        &self.value
    }

    /// Get the next node.
    unsafe fn next(&self) -> Option<ptr::NonNull<Self>> {
        let ptr = self.pointers.get() as *const _ as *const Option<ptr::NonNull<Self>>;
        ptr::read(ptr)
    }

    /// Set the next node.
    unsafe fn set_next(&mut self, node: Option<ptr::NonNull<Self>>) {
        let ptr = self.pointers.get() as *mut Option<ptr::NonNull<Self>>;
        ptr::write(ptr, node);
    }

    /// Take the next node.
    #[cfg(test)]
    unsafe fn take_next(&mut self) -> Option<ptr::NonNull<Self>> {
        let ptr = self.pointers.get() as *mut Option<ptr::NonNull<Self>>;
        ptr::replace(ptr, None)
    }

    /// Get the previous node.
    unsafe fn prev(&self) -> Option<ptr::NonNull<Self>> {
        let ptr = self.pointers.get() as *const _ as *const Option<ptr::NonNull<Self>>;
        let ptr = ptr.add(1);
        ptr::read(ptr)
    }

    /// Set the previous node.
    unsafe fn set_prev(&mut self, node: Option<ptr::NonNull<Self>>) {
        let ptr = self.pointers.get() as *mut Option<ptr::NonNull<Self>>;
        let ptr = ptr.add(1);
        ptr::write(ptr, node);
    }

    /// Take the previous node.
    unsafe fn take_prev(&mut self) -> Option<ptr::NonNull<Self>> {
        let ptr = self.pointers.get() as *mut Option<ptr::NonNull<Self>>;
        let ptr = ptr.add(1);
        ptr::replace(ptr, None)
    }

    /// Take both nodes at the same time.
    unsafe fn take_pair(&mut self) -> (Option<ptr::NonNull<Self>>, Option<ptr::NonNull<Self>>) {
        let next = self.pointers.get() as *mut Option<ptr::NonNull<Self>>;
        let prev = next.add(1);
        let next = ptr::replace(next, None);
        let prev = ptr::replace(prev, None);
        (next, prev)
    }
}

/// An intrusive linked list.
///
/// Because of the intrusive nature of the list, the list itself can only assert
/// that you have shared or exlusive access to the underlying link structure by
/// requiring `&self` or `&mut self`. It cannot however ensure that the returned
/// node is available for the given access node since it might be stored and
/// used somewhere else so this must be externally synchronized.
///
/// In terms of access to the nodes processed by this list, you can correctly
/// dereference the returned pointers shared or exclusively depending on the
/// signature of the function used in this list. If it takes `&self`, you can
/// correctly use methods such as [ptr::NonNull::as_ref]. Conversely if it takes
/// `&mut self`, you can use methods such as [ptr::NonNull::as_mut].
pub struct LinkedList<T> {
    head: Option<ptr::NonNull<Node<T>>>,
    tail: Option<ptr::NonNull<Node<T>>>,
}

impl<T> LinkedList<T> {
    /// Construct a new empty list.
    pub(crate) const fn new() -> Self {
        Self {
            head: None,
            tail: None,
        }
    }

    /// Push to the front of the linked list.
    ///
    /// Returns a boolean that if `true` indicates that this was the first
    /// element in the list.
    ///
    /// # Safety
    ///
    /// The soundness of manipulating the data in the list depends entirely on
    /// what was pushed. If you intend to mutate the data, you must push a
    /// pointer that is based out of something that was exclusively borrowed
    /// (example below).
    ///
    /// The caller also must ensure that the data pushed doesn't outlive its
    /// use.
    #[cfg_attr(feature = "tracing", tracing::instrument(level = "trace"))]
    pub(crate) unsafe fn push_front(&mut self, mut node: ptr::NonNull<Node<T>>) {
        debug_assert!(node.as_ref().next().is_none());
        debug_assert!(node.as_ref().prev().is_none());

        if let Some(mut head) = self.head.take() {
            node.as_mut().set_next(Some(head));
            head.as_mut().set_prev(Some(node));
            self.head = Some(node);
        } else {
            self.head = Some(node);
            self.tail = Some(node);
        }
    }

    /// Push to the front of the linked list.
    ///
    /// Returns a boolean that if `true` indicates that this was the first
    /// element in the list.
    ///
    /// # Safety
    ///
    /// The soundness of manipulating the data in the list depends entirely on
    /// what was pushed. If you intend to mutate the data, you must push a
    /// pointer that is based out of something that was exclusively borrowed
    /// (example below).
    ///
    /// The caller also must ensure that the data pushed doesn't outlive its
    /// use.
    #[cfg_attr(feature = "tracing", tracing::instrument(level = "trace"))]
    pub(crate) unsafe fn push_back(&mut self, mut node: ptr::NonNull<Node<T>>) {
        trace!(head = ?self.head, tail = ?self.tail, node = ?node, "push_back");

        debug_assert!(node.as_ref().next().is_none());
        debug_assert!(node.as_ref().prev().is_none());

        if let Some(mut tail) = self.tail.take() {
            node.as_mut().set_prev(Some(tail));
            tail.as_mut().set_next(Some(node));
            self.tail = Some(node);
        } else {
            self.head = Some(node);
            self.tail = Some(node);
        }
    }

    #[cfg(test)]
    #[cfg_attr(feature = "tracing", tracing::instrument(level = "trace"))]
    unsafe fn pop_front(&mut self) -> Option<ptr::NonNull<Node<T>>> {
        trace!(head = ?self.head, tail = ?self.tail, "pop_front");

        let mut head = self.head?;

        if let Some(mut next) = head.as_mut().take_next() {
            next.as_mut().set_prev(None);
            self.head = Some(next);
        } else {
            debug_assert_eq!(self.tail, Some(head));

            self.head = None;
            self.tail = None;
        }

        debug_assert!(head.as_ref().prev().is_none());
        debug_assert!(head.as_ref().next().is_none());
        Some(head)
    }

    /// Pop the back element from the list.
    #[cfg_attr(feature = "tracing", tracing::instrument(level = "trace"))]
    pub(crate) unsafe fn pop_back(&mut self) -> Option<ptr::NonNull<Node<T>>> {
        trace!(head = ?self.head, tail = ?self.tail, "pop_back");

        let mut tail = self.tail?;

        if let Some(mut prev) = tail.as_mut().take_prev() {
            prev.as_mut().set_next(None);
            self.tail = Some(prev);
        } else {
            debug_assert_eq!(self.head, Some(tail));

            self.head = None;
            self.tail = None;
        }

        debug_assert!(tail.as_ref().prev().is_none());
        debug_assert!(tail.as_ref().next().is_none());
        Some(tail)
    }

    /// Remove the specified node.
    #[cfg_attr(feature = "tracing", tracing::instrument(level = "trace"))]
    pub(crate) unsafe fn remove(&mut self, mut node: ptr::NonNull<Node<T>>) {
        trace!(head = ?self.head, tail = ?self.tail, node = ?node, "remove");

        let (next, prev) = node.as_mut().take_pair();

        if let Some(mut next) = next {
            next.as_mut().set_prev(prev);
        } else {
            debug_assert_eq!(self.tail, Some(node));
            self.tail = prev;
        }

        if let Some(mut prev) = prev {
            prev.as_mut().set_next(next);
        } else {
            debug_assert_eq!(self.head, Some(node));
            self.head = next;
        }
    }

    /// Mutably get the front of the list.
    ///
    /// This returns a raw pointer which can correctly be mutably accessed since
    /// the signature of this method ensures exclusive access to the list.
    #[cfg_attr(feature = "tracing", tracing::instrument(level = "trace"))]
    pub(crate) unsafe fn front_mut(&mut self) -> Option<ptr::NonNull<Node<T>>> {
        self.head
    }

    /// Test if the list is empty.
    pub(crate) fn is_empty(&self) -> bool {
        self.head.is_none()
    }
}

impl<T> fmt::Debug for LinkedList<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LinkedList")
            .field("head", &self.head)
            .field("tail", &self.tail)
            .finish()
    }
}

impl<T> ops::Deref for Node<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

impl<T> ops::DerefMut for Node<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.value
    }
}

#[cfg(test)]
mod tests {
    use std::ptr;

    use super::*;

    #[test]
    fn push_back() {
        let mut list = LinkedList::new();

        let mut a = Node::new(0);
        let mut b = Node::new(0);

        unsafe {
            list.push_back(ptr::NonNull::from(&mut a));
            list.push_back(ptr::NonNull::from(&mut b));

            let mut n = 1;

            while let Some(mut last) = list.pop_back() {
                **last.as_mut() += n;
                n <<= 1;
            }
        }

        assert_eq!(*a, 2);
        assert_eq!(*b, 1);
    }

    #[test]
    fn push_front() {
        let mut list = LinkedList::new();

        let mut a = Node::new(0);
        let mut b = Node::new(0);

        unsafe {
            list.push_front(ptr::NonNull::from(&mut a));
            list.push_front(ptr::NonNull::from(&mut b));

            let mut n = 1;

            while let Some(mut last) = list.pop_back() {
                **last.as_mut() += n;
                n <<= 1;
            }
        }

        assert_eq!(*a, 1);
        assert_eq!(*b, 2);
    }

    #[test]
    fn remove() {
        let mut list = LinkedList::new();

        let mut a = Node::new(0);
        let mut b = Node::new(0);
        let mut c = Node::new(0);
        let mut d = Node::new(0);

        unsafe {
            list.push_back(ptr::NonNull::from(&mut a));
            list.push_back(ptr::NonNull::from(&mut b));
            list.push_back(ptr::NonNull::from(&mut c));
            list.push_back(ptr::NonNull::from(&mut d));

            list.remove(ptr::NonNull::from(&mut b));
            list.remove(ptr::NonNull::from(&mut d));

            let mut n = 1;

            while let Some(mut last) = list.pop_back() {
                **last.as_mut() += n;
                n <<= 1;
            }
        }

        assert_eq!(*a, 2);
        assert_eq!(*b, 0);
        assert_eq!(*c, 1);
        assert_eq!(*d, 0);
    }

    #[test]
    fn front_mut() {
        let mut list = LinkedList::new();

        let mut a = Node::new(0);
        let mut b = Node::new(1);

        unsafe {
            list.push_back(ptr::NonNull::from(&mut a));
            list.push_back(ptr::NonNull::from(&mut b));

            let mut n = 1;

            while let Some(mut node) = list.pop_front() {
                **node.as_mut() += n;
                n <<= 1;
            }

            assert!(list.front_mut().is_none());
        }

        assert_eq!(*a, 1);
        assert_eq!(*b, 3);
    }

    #[test]
    fn pop_back() {
        let mut list = LinkedList::new();

        let mut a = Node::new(0);
        let mut b = Node::new(0);

        unsafe {
            list.push_back(ptr::NonNull::from(&mut a));
            list.push_back(ptr::NonNull::from(&mut b));

            let mut n = 1;

            while let Some(mut last) = list.pop_back() {
                **last.as_mut() += n;
                n <<= 1;
            }
        }

        assert_eq!(*a, 2);
        assert_eq!(*b, 1);
    }
}

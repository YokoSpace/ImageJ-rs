use ij_app::roi_edit::{drag_handle, hit_test, move_by, nudge, nudge_corner, Arrow, RoiHit};
use ij_bridge::JavaOracleBridge;
use ij_core::{ImageDocument, LineRoi, RectRoi, Roi};
use std::path::PathBuf;

#[test]
fn test_rect_handle_numbering_and_positions_mag1() {
    let rect = Roi::Rect(RectRoi::new(20, 30, 40, 50).unwrap());
    let mag = 1.0;
    let origin = (0.0, 0.0);

    // Java Roi.java line 1570: on screen > 1280 (standard desktop), margin=5, size=7+5=12, halfSize=6
    // sx1 = 20 - 6 = 14, sx1+size = 26
    // sy1 = 30 - 6 = 24, sy1+size = 36
    // sx3 = 60 - 6 = 54, sx3+size = 66
    // sy3 = 80 - 6 = 74, sy3+size = 86
    // sx2 = 14 + (54 - 14)/2 = 34
    // sy2 = 24 + (74 - 24)/2 = 49

    // Handle 0: Top-Left (20, 30)
    assert_eq!(hit_test(&rect, (20.0, 30.0), mag, origin), RoiHit::Handle(0));
    // Handle 1: Top-Center (40, 30)
    assert_eq!(hit_test(&rect, (40.0, 30.0), mag, origin), RoiHit::Handle(1));
    // Handle 2: Top-Right (60, 30)
    assert_eq!(hit_test(&rect, (60.0, 30.0), mag, origin), RoiHit::Handle(2));
    // Handle 3: Mid-Right (60, 55)
    assert_eq!(hit_test(&rect, (60.0, 55.0), mag, origin), RoiHit::Handle(3));
    // Handle 4: Bottom-Right (60, 80)
    assert_eq!(hit_test(&rect, (60.0, 80.0), mag, origin), RoiHit::Handle(4));
    // Handle 5: Bottom-Center (40, 80)
    assert_eq!(hit_test(&rect, (40.0, 80.0), mag, origin), RoiHit::Handle(5));
    // Handle 6: Bottom-Left (20, 80)
    assert_eq!(hit_test(&rect, (20.0, 80.0), mag, origin), RoiHit::Handle(6));
    // Handle 7: Mid-Left (20, 55)
    assert_eq!(hit_test(&rect, (20.0, 55.0), mag, origin), RoiHit::Handle(7));
}

#[test]
fn test_oval_handle_numbering_and_positions_mag1() {
    let oval = Roi::Oval(RectRoi::new(20, 30, 100, 100).unwrap());
    let mag = 1.0;
    let origin = (0.0, 0.0);

    // Java OvalRoi.java lines 377-400:
    // sw2 = (int)(0.14645 * 100) = 14
    // sh2 = (int)(0.14645 * 100) = 14
    // Top-Left corner handle 0 is at (20 + 14, 30 + 14) = (34, 44)
    assert_eq!(hit_test(&oval, (34.0, 44.0), mag, origin), RoiHit::Handle(0));
    // Top-Center handle 1 is at (70, 30)
    assert_eq!(hit_test(&oval, (70.0, 30.0), mag, origin), RoiHit::Handle(1));
    // Top-Right corner handle 2 is at (120 - 14, 30 + 14) = (106, 44)
    assert_eq!(hit_test(&oval, (106.0, 44.0), mag, origin), RoiHit::Handle(2));
    // Mid-Right handle 3 is at (120, 80)
    assert_eq!(hit_test(&oval, (120.0, 80.0), mag, origin), RoiHit::Handle(3));
    // Bottom-Right corner handle 4 is at (120 - 14, 130 - 14) = (106, 116)
    assert_eq!(hit_test(&oval, (106.0, 116.0), mag, origin), RoiHit::Handle(4));
    // Bottom-Center handle 5 is at (70, 130)
    assert_eq!(hit_test(&oval, (70.0, 130.0), mag, origin), RoiHit::Handle(5));
    // Bottom-Left corner handle 6 is at (20 + 14, 130 - 14) = (34, 116)
    assert_eq!(hit_test(&oval, (34.0, 116.0), mag, origin), RoiHit::Handle(6));
    // Mid-Left handle 7 is at (20, 80)
    assert_eq!(hit_test(&oval, (20.0, 80.0), mag, origin), RoiHit::Handle(7));
}

#[test]
fn test_line_handle_numbering_mag1() {
    let line = Roi::Line(LineRoi {
        x1: 10,
        y1: 20,
        x2: 50,
        y2: 80,
    });
    let mag = 1.0;
    let origin = (0.0, 0.0);

    // Handle 0: Start endpoint (10, 20)
    assert_eq!(hit_test(&line, (10.0, 20.0), mag, origin), RoiHit::Handle(0));
    // Handle 1: End endpoint (50, 80)
    assert_eq!(hit_test(&line, (50.0, 80.0), mag, origin), RoiHit::Handle(1));
    // Handle 2: Center midpoint (10 + 20, 20 + 30) = (30, 50)
    assert_eq!(hit_test(&line, (30.0, 50.0), mag, origin), RoiHit::Handle(2));
}

#[test]
fn test_magnifications_and_src_origin() {
    let rect = Roi::Rect(RectRoi::new(100, 100, 40, 40).unwrap());

    // mag = 2.0, origin = (50.0, 50.0)
    // screen x for 100 = (100 - 50) * 2 = 100
    // screen y for 100 = (100 - 50) * 2 = 100
    assert_eq!(
        hit_test(&rect, (100.0, 100.0), 2.0, (50.0, 50.0)),
        RoiHit::Handle(0)
    );
    // bottom-right handle 4: x=140, y=140 -> screen = (140 - 50) * 2 = 180
    assert_eq!(
        hit_test(&rect, (180.0, 180.0), 2.0, (50.0, 50.0)),
        RoiHit::Handle(4)
    );

    // mag = 4.0, origin = (80.0, 80.0)
    // screen x for 100 = (100 - 80) * 4 = 80
    // screen y for 100 = (100 - 80) * 4 = 80
    assert_eq!(
        hit_test(&rect, (80.0, 80.0), 4.0, (80.0, 80.0)),
        RoiHit::Handle(0)
    );
}

#[test]
fn test_inside_and_outside_hit() {
    let rect = Roi::Rect(RectRoi::new(20, 30, 40, 50).unwrap());
    // Inside rectangle center (40, 55)
    assert_eq!(hit_test(&rect, (40.0, 55.0), 1.0, (0.0, 0.0)), RoiHit::Inside);
    // Outside rectangle
    assert_eq!(hit_test(&rect, (10.0, 10.0), 1.0, (0.0, 0.0)), RoiHit::Outside);

    let oval = Roi::Oval(RectRoi::new(20, 30, 100, 100).unwrap());
    // Center (70, 80) is inside
    assert_eq!(hit_test(&oval, (70.0, 80.0), 1.0, (0.0, 0.0)), RoiHit::Inside);
    // Bounding box corner (21, 31) is outside the ellipse
    assert_eq!(hit_test(&oval, (21.0, 31.0), 1.0, (0.0, 0.0)), RoiHit::Outside);

    let line = Roi::Line(LineRoi {
        x1: 10,
        y1: 10,
        x2: 90,
        y2: 90,
    });
    // Far point is outside
    assert_eq!(hit_test(&line, (10.0, 90.0), 1.0, (0.0, 0.0)), RoiHit::Outside);
}

#[test]
fn test_move_by() {
    let rect = Roi::Rect(RectRoi::new(10, 20, 30, 40).unwrap());
    let moved = move_by(&rect, 5, -3);
    assert_eq!(moved.bounds(), (15, 17, 30, 40));

    let oval = Roi::Oval(RectRoi::new(10, 20, 30, 40).unwrap());
    let moved = move_by(&oval, -2, 4);
    assert_eq!(moved.bounds(), (8, 24, 30, 40));

    let line = Roi::Line(LineRoi {
        x1: 5,
        y1: 10,
        x2: 25,
        y2: 30,
    });
    let moved = move_by(&line, 10, 20);
    if let Roi::Line(l) = moved {
        assert_eq!((l.x1, l.y1, l.x2, l.y2), (15, 30, 35, 50));
    } else {
        panic!("expected Line");
    }
}

#[test]
fn test_drag_handle_rect() {
    let rect = Roi::Rect(RectRoi::new(20, 30, 40, 50).unwrap());

    // Drag handle 0 (top-left) to (15, 25)
    let r0 = drag_handle(&rect, 0, (15, 25));
    assert_eq!(r0.bounds(), (15, 25, 45, 55));

    // Drag handle 4 (bottom-right) to (80, 100)
    let r4 = drag_handle(&rect, 4, (80, 100));
    assert_eq!(r4.bounds(), (20, 30, 60, 70));

    // Drag handle 0 past bottom-right corner (ox=70, oy=90 while x2=60, y2=80)
    // Java Roi.java lines 990-997: if (x<x2) width=x2-x else {width=1; x=x2;}
    let r_flip = drag_handle(&rect, 0, (70, 90));
    assert_eq!(r_flip.bounds(), (60, 80, 1, 1));
}

#[test]
fn test_drag_handle_oval() {
    let oval = Roi::Oval(RectRoi::new(20, 30, 100, 100).unwrap());

    // Drag handle 3 (mid-right) to x=150
    let o3 = drag_handle(&oval, 3, (150, 80));
    assert_eq!(o3.bounds(), (20, 30, 130, 100));

    // Drag handle 1 (top-center) to y=10
    let o1 = drag_handle(&oval, 1, (70, 10));
    assert_eq!(o1.bounds(), (20, 10, 100, 120));
}

#[test]
fn test_drag_handle_line() {
    let line = Roi::Line(LineRoi {
        x1: 10,
        y1: 20,
        x2: 50,
        y2: 80,
    });

    // Drag handle 0 (endpoint 1) to (5, 15)
    let l0 = drag_handle(&line, 0, (5, 15));
    if let Roi::Line(l) = l0 {
        assert_eq!((l.x1, l.y1, l.x2, l.y2), (5, 15, 50, 80));
    } else {
        panic!("expected Line");
    }

    // Drag handle 1 (endpoint 2) to (60, 90)
    let l1 = drag_handle(&line, 1, (60, 90));
    if let Roi::Line(l) = l1 {
        assert_eq!((l.x1, l.y1, l.x2, l.y2), (10, 20, 60, 90));
    } else {
        panic!("expected Line");
    }

    // Drag handle 2 (center midpoint) to (40, 60)
    // Old center is (30, 50), so dx=10, dy=10
    let l2 = drag_handle(&line, 2, (40, 60));
    if let Roi::Line(l) = l2 {
        assert_eq!((l.x1, l.y1, l.x2, l.y2), (20, 30, 60, 90));
    } else {
        panic!("expected Line");
    }
}

#[test]
fn test_boundary_nudge() {
    let rect_at_origin = Roi::Rect(RectRoi::new(0, 0, 20, 20).unwrap());
    // Up & Left should clamp to 0
    let nudged = nudge(&rect_at_origin, Arrow::Up, 100, 100);
    assert_eq!(nudged.bounds(), (0, 0, 20, 20));
    let nudged = nudge(&rect_at_origin, Arrow::Left, 100, 100);
    assert_eq!(nudged.bounds(), (0, 0, 20, 20));

    // Right & Down within bounds
    let nudged = nudge(&rect_at_origin, Arrow::Right, 100, 100);
    assert_eq!(nudged.bounds(), (1, 0, 20, 20));
    let nudged = nudge(&rect_at_origin, Arrow::Down, 100, 100);
    assert_eq!(nudged.bounds(), (0, 1, 20, 20));

    // Boundary at right edge: x=80, w=20, img_w=100 -> (80 + 20) == 100 >= 100
    let rect_at_edge = Roi::Rect(RectRoi::new(80, 80, 20, 20).unwrap());
    let nudged = nudge(&rect_at_edge, Arrow::Right, 100, 100);
    assert_eq!(nudged.bounds(), (80, 80, 20, 20));
    let nudged = nudge(&rect_at_edge, Arrow::Down, 100, 100);
    assert_eq!(nudged.bounds(), (80, 80, 20, 20));
}

#[test]
fn test_nudge_corner() {
    let rect = Roi::Rect(RectRoi::new(10, 10, 20, 20).unwrap());

    // Up decreases height
    let nc_up = nudge_corner(&rect, Arrow::Up, 100, 100);
    assert_eq!(nc_up.bounds(), (10, 10, 20, 19));

    // Left decreases width
    let nc_left = nudge_corner(&rect, Arrow::Left, 100, 100);
    assert_eq!(nc_left.bounds(), (10, 10, 19, 20));

    // Right increases width
    let nc_right = nudge_corner(&rect, Arrow::Right, 100, 100);
    assert_eq!(nc_right.bounds(), (10, 10, 21, 20));

    // Down increases height
    let nc_down = nudge_corner(&rect, Arrow::Down, 100, 100);
    assert_eq!(nc_down.bounds(), (10, 10, 20, 21));

    // Minimum size 1
    let small = Roi::Rect(RectRoi::new(10, 10, 1, 1).unwrap());
    let nc_min = nudge_corner(&small, Arrow::Up, 100, 100);
    assert_eq!(nc_min.bounds(), (10, 10, 1, 1));
    let nc_min = nudge_corner(&small, Arrow::Left, 100, 100);
    assert_eq!(nc_min.bounds(), (10, 10, 1, 1));

    // Boundary clamping at image edge: x=10, w=90, img_w=100 -> x+w=100
    let at_edge = Roi::Rect(RectRoi::new(10, 10, 90, 90).unwrap());
    let nc_edge = nudge_corner(&at_edge, Arrow::Right, 100, 100);
    assert_eq!(nc_edge.bounds(), (10, 10, 90, 90));
}

#[test]
fn test_oracle_parity_nudge_and_corner() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("repo root")
        .to_path_buf();
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root).expect("bridge");
    let doc = ImageDocument::new_ramp_8bit("ramp", 100, 100).unwrap();

    // Rect nudge parity
    let out = bridge
        .run_macro_on_image(
            &doc,
            "makeRectangle(20, 30, 40, 50);\n\
             print(call(\"tools.oracle.UiProbe.nudge\", \"right\"));\n\
             print(call(\"tools.oracle.UiProbe.nudge\", \"down\"));\n\
             print(call(\"tools.oracle.UiProbe.nudgeCorner\", \"right\"));\n\
             print(call(\"tools.oracle.UiProbe.nudgeCorner\", \"up\"));",
        )
        .expect("oracle macro failed");

    let lines: Vec<&str> = out.stdout.lines().collect();
    assert_eq!(lines.len(), 4, "expected 4 lines from oracle: {:?}", lines);

    let mut rect = Roi::Rect(RectRoi::new(20, 30, 40, 50).unwrap());
    rect = nudge(&rect, Arrow::Right, 100, 100);
    assert_eq!(
        format!("{},{},{},{}", rect.bounds().0, rect.bounds().1, rect.bounds().2, rect.bounds().3),
        lines[0]
    );

    rect = nudge(&rect, Arrow::Down, 100, 100);
    assert_eq!(
        format!("{},{},{},{}", rect.bounds().0, rect.bounds().1, rect.bounds().2, rect.bounds().3),
        lines[1]
    );

    rect = nudge_corner(&rect, Arrow::Right, 100, 100);
    assert_eq!(
        format!("{},{},{},{}", rect.bounds().0, rect.bounds().1, rect.bounds().2, rect.bounds().3),
        lines[2]
    );

    rect = nudge_corner(&rect, Arrow::Up, 100, 100);
    assert_eq!(
        format!("{},{},{},{}", rect.bounds().0, rect.bounds().1, rect.bounds().2, rect.bounds().3),
        lines[3]
    );

    // Line nudge oracle parity
    let out_line = bridge
        .run_macro_on_image(
            &doc,
            "makeLine(10, 20, 50, 80);\n\
             print(call(\"tools.oracle.UiProbe.nudge\", \"right\"));\n\
             print(call(\"tools.oracle.UiProbe.nudge\", \"up\"));",
        )
        .expect("oracle macro line nudge failed");
    let line_lines: Vec<&str> = out_line.stdout.lines().collect();
    assert_eq!(line_lines.len(), 2);

    let mut line = Roi::Line(LineRoi { x1: 10, y1: 20, x2: 50, y2: 80 });
    line = nudge(&line, Arrow::Right, 100, 100);
    if let Roi::Line(l) = line {
        assert_eq!(format!("{},{},{},{}", l.x1, l.y1, l.x2, l.y2), line_lines[0]);
    }

    line = nudge(&line, Arrow::Up, 100, 100);
    if let Roi::Line(l) = line {
        assert_eq!(format!("{},{},{},{}", l.x1, l.y1, l.x2, l.y2), line_lines[1]);
    }
}

#[test]
fn test_line_boundary_nudge_and_corner() {
    let line_origin = Roi::Line(LineRoi { x1: 0, y1: 5, x2: 10, y2: 15 });
    // Left clamps to 0
    let n = nudge(&line_origin, Arrow::Left, 100, 100);
    if let Roi::Line(l) = n {
        assert_eq!((l.x1, l.y1, l.x2, l.y2), (0, 5, 10, 15));
    }

    // Top clamps to 0
    let line_top = Roi::Line(LineRoi { x1: 5, y1: 0, x2: 15, y2: 10 });
    let n = nudge(&line_top, Arrow::Up, 100, 100);
    if let Roi::Line(l) = n {
        assert_eq!((l.x1, l.y1, l.x2, l.y2), (5, 0, 15, 10));
    }

    // Bottom-right clamp
    let line_bottom = Roi::Line(LineRoi { x1: 70, y1: 80, x2: 85, y2: 100 });
    let n = nudge(&line_bottom, Arrow::Down, 100, 100);
    if let Roi::Line(l) = n {
        assert_eq!((l.x1, l.y1, l.x2, l.y2), (70, 80, 85, 100));
    }

    // nudge_corner clamps to [0, max-1]
    let line_corner = Roi::Line(LineRoi { x1: 10, y1: 10, x2: 99, y2: 99 });
    let nc = nudge_corner(&line_corner, Arrow::Right, 100, 100);
    if let Roi::Line(l) = nc {
        assert_eq!((l.x1, l.y1, l.x2, l.y2), (10, 10, 99, 99));
    }
    let nc = nudge_corner(&line_corner, Arrow::Down, 100, 100);
    if let Roi::Line(l) = nc {
        assert_eq!((l.x1, l.y1, l.x2, l.y2), (10, 10, 99, 99));
    }
}

